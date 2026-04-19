#ifndef _POSIX_C_SOURCE
#define _POSIX_C_SOURCE 199309L
#endif

#include "tracehash_c.h"

#include <pthread.h>
#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define FNV_OFFSET UINT64_C(0xcbf29ce484222325)
#define FNV_PRIME UINT64_C(0x100000001b3)

static FILE *tracehash_file = NULL;
static const char *tracehash_side = "c";
static const char *tracehash_run_id = "default";
static pthread_mutex_t tracehash_mutex = PTHREAD_MUTEX_INITIALIZER;
static uint64_t tracehash_seq = 0;
static int tracehash_initialized = 0;
static int tracehash_values = 0;

/* Deep-mode state. Activated when TRACEHASH_DEEP_DIR is set. */
static char *tracehash_deep_dir = NULL;
static uint64_t tracehash_deep_first_n = 100;
static int tracehash_deep_mode_all = 0;
static int tracehash_deep_enabled = 0;

typedef struct TraceHashDeepFile {
  char *function;
  FILE *file;
  pthread_mutex_t mutex;
  uint32_t seq;
  struct TraceHashDeepFile *next;
} TraceHashDeepFile;

static TraceHashDeepFile *tracehash_deep_files = NULL;
static pthread_mutex_t tracehash_deep_registry_mutex = PTHREAD_MUTEX_INITIALIZER;

static void hash_u8(uint64_t *hash, uint8_t value) {
  *hash ^= value;
  *hash *= FNV_PRIME;
}

static void hash_bytes(uint64_t *hash, const uint8_t *bytes, size_t len) {
  size_t i;
  for (i = 0; i < len; i++) hash_u8(hash, bytes[i]);
}

static void hash_u32(uint64_t *hash, uint32_t value) {
  uint8_t bytes[4];
  bytes[0] = (uint8_t)(value);
  bytes[1] = (uint8_t)(value >> 8);
  bytes[2] = (uint8_t)(value >> 16);
  bytes[3] = (uint8_t)(value >> 24);
  hash_bytes(hash, bytes, 4);
}

static void hash_u64(uint64_t *hash, uint64_t value) {
  uint8_t bytes[8];
  int i;
  for (i = 0; i < 8; i++) bytes[i] = (uint8_t)(value >> (8 * i));
  hash_bytes(hash, bytes, 8);
}

static void hash_f32(uint64_t *hash, float value) {
  union {
    float f;
    uint32_t u;
  } bits;
  bits.f = value;
  hash_u32(hash, bits.u);
}

static void hash_f64(uint64_t *hash, double value) {
  union {
    double f;
    uint64_t u;
  } bits;
  bits.f = value;
  hash_u64(hash, bits.u);
}

static void hash_str(uint64_t *hash, const char *value) {
  size_t len = strlen(value);
  hash_u64(hash, (uint64_t)len);
  hash_bytes(hash, (const uint8_t *)value, len);
}

static void tracehash_append_value(TraceHashCall *call, const char *fmt, ...) {
  va_list ap;
  va_list ap2;
  int needed;
  size_t prefix;
  if (!call->active || !call->values_enabled) return;

  prefix = call->values_len == 0 ? 0 : 1;
  va_start(ap, fmt);
  va_copy(ap2, ap);
  needed = vsnprintf(NULL, 0, fmt, ap);
  va_end(ap);
  if (needed < 0) {
    va_end(ap2);
    return;
  }
  if (call->values_len + prefix + (size_t)needed + 1 > call->values_cap) {
    size_t new_cap = call->values_cap == 0 ? 128 : call->values_cap;
    char *new_values;
    while (call->values_len + prefix + (size_t)needed + 1 > new_cap) new_cap *= 2;
    new_values = (char *)realloc(call->values, new_cap);
    if (new_values == NULL) {
      free(call->values);
      call->values = NULL;
      call->values_len = 0;
      call->values_cap = 0;
      call->values_enabled = 0;
      va_end(ap2);
      return;
    }
    call->values = new_values;
    call->values_cap = new_cap;
  }
  if (prefix) call->values[call->values_len++] = ';';
  vsnprintf(call->values + call->values_len, call->values_cap - call->values_len, fmt, ap2);
  va_end(ap2);
  call->values_len += (size_t)needed;
}

static void hash_trace_u64(uint64_t *hash, uint64_t value) {
  hash_u8(hash, 'U');
  hash_u64(hash, value);
}

static void hash_trace_bool(uint64_t *hash, int value) {
  hash_u8(hash, 'B');
  hash_u8(hash, value ? 1 : 0);
}

static void hash_trace_f32(uint64_t *hash, float value) {
  hash_u8(hash, 'F');
  hash_f32(hash, value);
}

static void hash_trace_f64(uint64_t *hash, double value) {
  hash_u8(hash, 'D');
  hash_f64(hash, value);
}

static void hash_trace_bytes(uint64_t *hash, const void *ptr, size_t len) {
  hash_u8(hash, 'Y');
  hash_u64(hash, (uint64_t)len);
  hash_bytes(hash, (const uint8_t *)ptr, len);
}

static int64_t quantize_f32(float value, float quantum) {
  union {
    float f;
    uint32_t u;
  } bits;
  float scaled;
  if (isnan(value)) return INT64_MIN;
  if (value == INFINITY) return INT64_MAX;
  if (value == -INFINITY) return INT64_MIN + 1;
  if (quantum > 0.0f) {
    scaled = value / quantum;
    return scaled >= 0.0f ? (int64_t)(scaled + 0.5f) : (int64_t)(scaled - 0.5f);
  }
  bits.f = value;
  return (int64_t)bits.u;
}

static uint64_t now_ns(void) {
  struct timespec ts;
  if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return 0;
  return (uint64_t)ts.tv_sec * UINT64_C(1000000000) + (uint64_t)ts.tv_nsec;
}

static void tracehash_init(void) {
  const char *out;
  const char *deep_dir;
  const char *deep_mode;
  if (tracehash_initialized) return;
  tracehash_initialized = 1;
  out = getenv("TRACEHASH_OUT");
  if (out != NULL && out[0] != '\0') {
    tracehash_file = fopen(out, "w");
  }
  if (getenv("TRACEHASH_SIDE") != NULL) tracehash_side = getenv("TRACEHASH_SIDE");
  if (getenv("TRACEHASH_RUN_ID") != NULL) tracehash_run_id = getenv("TRACEHASH_RUN_ID");
  if (getenv("TRACEHASH_VALUES") != NULL &&
      getenv("TRACEHASH_VALUES")[0] != '\0' &&
      strcmp(getenv("TRACEHASH_VALUES"), "0") != 0)
    tracehash_values = 1;

  deep_dir = getenv("TRACEHASH_DEEP_DIR");
  if (deep_dir != NULL && deep_dir[0] != '\0') {
    size_t n = strlen(deep_dir) + 1;
    tracehash_deep_dir = (char *)malloc(n);
    if (tracehash_deep_dir != NULL) {
      memcpy(tracehash_deep_dir, deep_dir, n);
      /* Best-effort mkdir; ignore EEXIST and other soft errors. */
      {
        char cmd[512];
        snprintf(cmd, sizeof(cmd), "mkdir -p '%s'", tracehash_deep_dir);
        (void)system(cmd);
      }
      tracehash_deep_enabled = 1;
    }
  }
  if (tracehash_deep_enabled) {
    deep_mode = getenv("TRACEHASH_DEEP_MODE");
    if (deep_mode != NULL && deep_mode[0] != '\0') {
      if (strcmp(deep_mode, "all") == 0) {
        tracehash_deep_mode_all = 1;
      } else if (strncmp(deep_mode, "first:", 6) == 0) {
        tracehash_deep_first_n = strtoull(deep_mode + 6, NULL, 10);
      }
      /* Unknown modes fall back to the first:100 default. */
    }
  }
}

TraceHashCall tracehash_begin(const char *function, const char *file, int line) {
  TraceHashCall call;
  pthread_mutex_lock(&tracehash_mutex);
  tracehash_init();
  pthread_mutex_unlock(&tracehash_mutex);

  call.function = function;
  call.file = file;
  call.line = line;
  call.input_hash = FNV_OFFSET;
  call.output_hash = FNV_OFFSET;
  call.input_len = 0;
  call.output_len = 0;
  call.start_ns = now_ns();
  call.values = NULL;
  call.values_len = 0;
  call.values_cap = 0;
  call.values_enabled = tracehash_values;
  call.active = (tracehash_file != NULL) || tracehash_deep_enabled;
  call.deep_active = tracehash_deep_enabled;
  call.deep_in_buf = NULL;
  call.deep_in_len = 0;
  call.deep_in_cap = 0;
  call.deep_in_count = 0;
  call.input_counter = 0;
  call.deep_out_buf = NULL;
  call.deep_out_len = 0;
  call.deep_out_cap = 0;
  call.deep_out_count = 0;
  call.output_counter = 0;
  if (call.active) {
    hash_str(&call.input_hash, function);
    hash_str(&call.output_hash, function);
  }
  return call;
}

/* -------------------------------------------------------------------------
 * Deep-mode byte-buffer helpers. Each input/output on a call is encoded
 * as: name_len (u32 LE) + name bytes + value_tag (u8) + value bytes.
 * The buffer is assembled in place, then drained at finish() time into the
 * per-function dclog file.
 * ----------------------------------------------------------------------- */

static int deep_buf_reserve(TraceHashCall *call, uint8_t **buf, size_t *len, size_t *cap, size_t want) {
  size_t need = *len + want;
  size_t new_cap;
  uint8_t *p;
  if (need <= *cap) return 1;
  new_cap = *cap == 0 ? 128 : *cap;
  while (need > new_cap) new_cap *= 2;
  p = (uint8_t *)realloc(*buf, new_cap);
  if (p == NULL) {
    call->deep_active = 0;
    return 0;
  }
  *buf = p;
  *cap = new_cap;
  return 1;
}

static void deep_buf_u8(TraceHashCall *call, uint8_t **buf, size_t *len, size_t *cap, uint8_t v) {
  if (!deep_buf_reserve(call, buf, len, cap, 1)) return;
  (*buf)[(*len)++] = v;
}

static void deep_buf_u32le(TraceHashCall *call, uint8_t **buf, size_t *len, size_t *cap, uint32_t v) {
  if (!deep_buf_reserve(call, buf, len, cap, 4)) return;
  (*buf)[(*len)++] = (uint8_t)(v);
  (*buf)[(*len)++] = (uint8_t)(v >> 8);
  (*buf)[(*len)++] = (uint8_t)(v >> 16);
  (*buf)[(*len)++] = (uint8_t)(v >> 24);
}

static void deep_buf_u64le(TraceHashCall *call, uint8_t **buf, size_t *len, size_t *cap, uint64_t v) {
  int i;
  if (!deep_buf_reserve(call, buf, len, cap, 8)) return;
  for (i = 0; i < 8; i++) (*buf)[(*len)++] = (uint8_t)(v >> (8 * i));
}

static void deep_buf_bytes(TraceHashCall *call, uint8_t **buf, size_t *len, size_t *cap,
                           const uint8_t *src, size_t n) {
  if (!deep_buf_reserve(call, buf, len, cap, n)) return;
  memcpy(*buf + *len, src, n);
  *len += n;
}

static void deep_buf_str(TraceHashCall *call, uint8_t **buf, size_t *len, size_t *cap, const char *s) {
  size_t n = strlen(s);
  deep_buf_u32le(call, buf, len, cap, (uint32_t)n);
  deep_buf_bytes(call, buf, len, cap, (const uint8_t *)s, n);
}

/* Tag bytes — keep in lockstep with `src/spec/tags.rs`. */
#define DEEP_TAG_I64   0x04
#define DEEP_TAG_U64   0x08
#define DEEP_TAG_F32   0x09
#define DEEP_TAG_F64   0x0A
#define DEEP_TAG_BOOL  0x0B
#define DEEP_TAG_BYTES 0x0D

static void deep_begin_input(TraceHashCall *call, const char *name) {
  char auto_name[16];
  uint32_t idx = call->input_counter++;
  if (name == NULL) {
    snprintf(auto_name, sizeof(auto_name), "in%u", (unsigned)idx);
    name = auto_name;
  }
  deep_buf_str(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, name);
  call->deep_in_count++;
}

static void deep_begin_output(TraceHashCall *call, const char *name) {
  char auto_name[16];
  uint32_t idx = call->output_counter++;
  if (name == NULL) {
    snprintf(auto_name, sizeof(auto_name), "out%u", (unsigned)idx);
    name = auto_name;
  }
  deep_buf_str(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, name);
  call->deep_out_count++;
}

static void deep_capture_in_u64(TraceHashCall *call, const char *name, uint64_t value) {
  if (!call->deep_active) return;
  deep_begin_input(call, name);
  deep_buf_u8(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, DEEP_TAG_U64);
  deep_buf_u64le(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, value);
}

static void deep_capture_in_i64(TraceHashCall *call, const char *name, int64_t value) {
  if (!call->deep_active) return;
  deep_begin_input(call, name);
  deep_buf_u8(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, DEEP_TAG_I64);
  deep_buf_u64le(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, (uint64_t)value);
}

static void deep_capture_in_bool(TraceHashCall *call, const char *name, int value) {
  if (!call->deep_active) return;
  deep_begin_input(call, name);
  deep_buf_u8(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, DEEP_TAG_BOOL);
  deep_buf_u8(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, value ? 1 : 0);
}

static void deep_capture_in_f32(TraceHashCall *call, const char *name, float value) {
  union { float f; uint32_t u; } bits;
  if (!call->deep_active) return;
  bits.f = value;
  deep_begin_input(call, name);
  deep_buf_u8(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, DEEP_TAG_F32);
  deep_buf_u32le(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, bits.u);
}

static void deep_capture_in_f64(TraceHashCall *call, const char *name, double value) {
  union { double f; uint64_t u; } bits;
  if (!call->deep_active) return;
  bits.f = value;
  deep_begin_input(call, name);
  deep_buf_u8(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, DEEP_TAG_F64);
  deep_buf_u64le(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, bits.u);
}

static void deep_capture_in_bytes(TraceHashCall *call, const char *name, const void *ptr, size_t len) {
  if (!call->deep_active) return;
  deep_begin_input(call, name);
  deep_buf_u8(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, DEEP_TAG_BYTES);
  deep_buf_u32le(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap, (uint32_t)len);
  deep_buf_bytes(call, &call->deep_in_buf, &call->deep_in_len, &call->deep_in_cap,
                 (const uint8_t *)ptr, len);
}

static void deep_capture_out_u64(TraceHashCall *call, const char *name, uint64_t value) {
  if (!call->deep_active) return;
  deep_begin_output(call, name);
  deep_buf_u8(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, DEEP_TAG_U64);
  deep_buf_u64le(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, value);
}

static void deep_capture_out_i64(TraceHashCall *call, const char *name, int64_t value) {
  if (!call->deep_active) return;
  deep_begin_output(call, name);
  deep_buf_u8(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, DEEP_TAG_I64);
  deep_buf_u64le(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, (uint64_t)value);
}

static void deep_capture_out_bool(TraceHashCall *call, const char *name, int value) {
  if (!call->deep_active) return;
  deep_begin_output(call, name);
  deep_buf_u8(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, DEEP_TAG_BOOL);
  deep_buf_u8(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, value ? 1 : 0);
}

static void deep_capture_out_f32(TraceHashCall *call, const char *name, float value) {
  union { float f; uint32_t u; } bits;
  if (!call->deep_active) return;
  bits.f = value;
  deep_begin_output(call, name);
  deep_buf_u8(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, DEEP_TAG_F32);
  deep_buf_u32le(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, bits.u);
}

static void deep_capture_out_f64(TraceHashCall *call, const char *name, double value) {
  union { double f; uint64_t u; } bits;
  if (!call->deep_active) return;
  bits.f = value;
  deep_begin_output(call, name);
  deep_buf_u8(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, DEEP_TAG_F64);
  deep_buf_u64le(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, bits.u);
}

static void deep_capture_out_bytes(TraceHashCall *call, const char *name, const void *ptr, size_t len) {
  if (!call->deep_active) return;
  deep_begin_output(call, name);
  deep_buf_u8(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, DEEP_TAG_BYTES);
  deep_buf_u32le(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap, (uint32_t)len);
  deep_buf_bytes(call, &call->deep_out_buf, &call->deep_out_len, &call->deep_out_cap,
                 (const uint8_t *)ptr, len);
}

void tracehash_input_u64(TraceHashCall *call, uint64_t value) {
  if (!call->active) return;
  hash_trace_u64(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IU64=%lu", (unsigned long)value);
  deep_capture_in_u64(call, NULL, value);
}

void tracehash_input_i64(TraceHashCall *call, int64_t value) {
  if (!call->active) return;
  hash_trace_u64(&call->input_hash, (uint64_t)value);
  call->input_len++;
  tracehash_append_value(call, "IU64=%lu", (unsigned long)(uint64_t)value);
  deep_capture_in_i64(call, NULL, value);
}

void tracehash_input_bool(TraceHashCall *call, int value) {
  if (!call->active) return;
  hash_trace_bool(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IBOOL=%d", value ? 1 : 0);
  deep_capture_in_bool(call, NULL, value);
}

void tracehash_input_f32(TraceHashCall *call, float value) {
  if (!call->active) return;
  hash_trace_f32(&call->input_hash, value);
  call->input_len++;
  {
    union { float f; uint32_t u; } bits;
    bits.f = value;
    tracehash_append_value(call, "IF32=%08x/%.9e", bits.u, value);
  }
  deep_capture_in_f32(call, NULL, value);
}

void tracehash_input_f64(TraceHashCall *call, double value) {
  if (!call->active) return;
  hash_trace_f64(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IF64=%.17e", value);
  deep_capture_in_f64(call, NULL, value);
}

void tracehash_input_f32_quant(TraceHashCall *call, float value, float quantum) {
  union {
    float f;
    uint32_t u;
  } qbits;
  int64_t q;
  if (!call->active) return;
  qbits.f = quantum;
  q = quantize_f32(value, quantum);
  hash_u8(&call->input_hash, 'Q');
  hash_u32(&call->input_hash, qbits.u);
  hash_u64(&call->input_hash, (uint64_t)q);
  call->input_len++;
  tracehash_append_value(call, "IF32Q=%08x/%ld", qbits.u, (long)q);
  deep_capture_in_f32(call, NULL, value);
}

void tracehash_input_bytes(TraceHashCall *call, const void *ptr, size_t len) {
  if (!call->active) return;
  hash_trace_bytes(&call->input_hash, ptr, len);
  call->input_len += (uint64_t)len;
  {
    uint64_t bytes_hash = FNV_OFFSET;
    hash_bytes(&bytes_hash, (const uint8_t *)ptr, len);
    tracehash_append_value(call, "IBYTES=%lu:%016lx", (unsigned long)len, (unsigned long)bytes_hash);
  }
  deep_capture_in_bytes(call, NULL, ptr, len);
}

void tracehash_output_u64(TraceHashCall *call, uint64_t value) {
  if (!call->active) return;
  hash_trace_u64(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OU64=%lu", (unsigned long)value);
  deep_capture_out_u64(call, NULL, value);
}

void tracehash_output_i64(TraceHashCall *call, int64_t value) {
  if (!call->active) return;
  hash_trace_u64(&call->output_hash, (uint64_t)value);
  call->output_len++;
  tracehash_append_value(call, "OU64=%lu", (unsigned long)(uint64_t)value);
  deep_capture_out_i64(call, NULL, value);
}

void tracehash_output_bool(TraceHashCall *call, int value) {
  if (!call->active) return;
  hash_trace_bool(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OBOOL=%d", value ? 1 : 0);
  deep_capture_out_bool(call, NULL, value);
}

void tracehash_output_f32(TraceHashCall *call, float value) {
  if (!call->active) return;
  hash_trace_f32(&call->output_hash, value);
  call->output_len++;
  {
    union { float f; uint32_t u; } bits;
    bits.f = value;
    tracehash_append_value(call, "OF32=%08x/%.9e", bits.u, value);
  }
  deep_capture_out_f32(call, NULL, value);
}

void tracehash_output_f64(TraceHashCall *call, double value) {
  if (!call->active) return;
  hash_trace_f64(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OF64=%.17e", value);
  deep_capture_out_f64(call, NULL, value);
}

void tracehash_output_f32_quant(TraceHashCall *call, float value, float quantum) {
  union {
    float f;
    uint32_t u;
  } qbits;
  int64_t q;
  if (!call->active) return;
  qbits.f = quantum;
  q = quantize_f32(value, quantum);
  hash_u8(&call->output_hash, 'Q');
  hash_u32(&call->output_hash, qbits.u);
  hash_u64(&call->output_hash, (uint64_t)q);
  call->output_len++;
  tracehash_append_value(call, "OF32Q=%08x/%ld", qbits.u, (long)q);
  deep_capture_out_f32(call, NULL, value);
}

void tracehash_output_bytes(TraceHashCall *call, const void *ptr, size_t len) {
  if (!call->active) return;
  hash_trace_bytes(&call->output_hash, ptr, len);
  call->output_len += (uint64_t)len;
  {
    uint64_t bytes_hash = FNV_OFFSET;
    hash_bytes(&bytes_hash, (const uint8_t *)ptr, len);
    tracehash_append_value(call, "OBYTES=%lu:%016lx", (unsigned long)len, (unsigned long)bytes_hash);
  }
  deep_capture_out_bytes(call, NULL, ptr, len);
}

static void hash_field_prefix(uint64_t *hash, const char *field) {
  hash_u8(hash, 'G');
  hash_str(hash, field);
}

void tracehash_input_field_u64(TraceHashCall *call, const char *field, uint64_t value) {
  if (!call->active) return;
  hash_field_prefix(&call->input_hash, field);
  hash_trace_u64(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IFIELD=%s", field);
}

void tracehash_input_field_i64(TraceHashCall *call, const char *field, int64_t value) {
  tracehash_input_field_u64(call, field, (uint64_t)value);
}

void tracehash_input_field_bool(TraceHashCall *call, const char *field, int value) {
  if (!call->active) return;
  hash_field_prefix(&call->input_hash, field);
  hash_trace_bool(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IFIELD=%s", field);
}

void tracehash_input_field_f32(TraceHashCall *call, const char *field, float value) {
  if (!call->active) return;
  hash_field_prefix(&call->input_hash, field);
  hash_trace_f32(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IFIELD=%s", field);
}

void tracehash_input_field_f64(TraceHashCall *call, const char *field, double value) {
  if (!call->active) return;
  hash_field_prefix(&call->input_hash, field);
  hash_trace_f64(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IFIELD=%s", field);
}

void tracehash_output_field_u64(TraceHashCall *call, const char *field, uint64_t value) {
  if (!call->active) return;
  hash_field_prefix(&call->output_hash, field);
  hash_trace_u64(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OFIELD=%s", field);
}

void tracehash_output_field_i64(TraceHashCall *call, const char *field, int64_t value) {
  tracehash_output_field_u64(call, field, (uint64_t)value);
}

void tracehash_output_field_bool(TraceHashCall *call, const char *field, int value) {
  if (!call->active) return;
  hash_field_prefix(&call->output_hash, field);
  hash_trace_bool(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OFIELD=%s", field);
}

void tracehash_output_field_f32(TraceHashCall *call, const char *field, float value) {
  if (!call->active) return;
  hash_field_prefix(&call->output_hash, field);
  hash_trace_f32(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OFIELD=%s", field);
}

void tracehash_output_field_f64(TraceHashCall *call, const char *field, double value) {
  if (!call->active) return;
  hash_field_prefix(&call->output_hash, field);
  hash_trace_f64(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OFIELD=%s", field);
}

void tracehash_input_struct_begin(TraceHashCall *call, const char *name) {
  if (!call->active) return;
  hash_u8(&call->input_hash, 'V');
  hash_str(&call->input_hash, name);
  call->input_len++;
}

void tracehash_output_struct_begin(TraceHashCall *call, const char *name) {
  if (!call->active) return;
  hash_u8(&call->output_hash, 'V');
  hash_str(&call->output_hash, name);
  call->output_len++;
}

void tracehash_input_struct_field_u64(TraceHashCall *call, const char *field, uint64_t value) {
  if (!call->active) return;
  hash_str(&call->input_hash, field);
  hash_trace_u64(&call->input_hash, value);
}

void tracehash_input_struct_field_i64(TraceHashCall *call, const char *field, int64_t value) {
  tracehash_input_struct_field_u64(call, field, (uint64_t)value);
}

void tracehash_input_struct_field_bool(TraceHashCall *call, const char *field, int value) {
  if (!call->active) return;
  hash_str(&call->input_hash, field);
  hash_trace_bool(&call->input_hash, value);
}

void tracehash_input_struct_field_f32(TraceHashCall *call, const char *field, float value) {
  if (!call->active) return;
  hash_str(&call->input_hash, field);
  hash_trace_f32(&call->input_hash, value);
}

void tracehash_input_struct_field_f64(TraceHashCall *call, const char *field, double value) {
  if (!call->active) return;
  hash_str(&call->input_hash, field);
  hash_trace_f64(&call->input_hash, value);
}

void tracehash_input_struct_field_bytes(TraceHashCall *call, const char *field, const void *ptr, size_t len) {
  if (!call->active) return;
  hash_str(&call->input_hash, field);
  hash_trace_bytes(&call->input_hash, ptr, len);
}

void tracehash_output_struct_field_u64(TraceHashCall *call, const char *field, uint64_t value) {
  if (!call->active) return;
  hash_str(&call->output_hash, field);
  hash_trace_u64(&call->output_hash, value);
}

void tracehash_output_struct_field_i64(TraceHashCall *call, const char *field, int64_t value) {
  tracehash_output_struct_field_u64(call, field, (uint64_t)value);
}

void tracehash_output_struct_field_bool(TraceHashCall *call, const char *field, int value) {
  if (!call->active) return;
  hash_str(&call->output_hash, field);
  hash_trace_bool(&call->output_hash, value);
}

void tracehash_output_struct_field_f32(TraceHashCall *call, const char *field, float value) {
  if (!call->active) return;
  hash_str(&call->output_hash, field);
  hash_trace_f32(&call->output_hash, value);
}

void tracehash_output_struct_field_f64(TraceHashCall *call, const char *field, double value) {
  if (!call->active) return;
  hash_str(&call->output_hash, field);
  hash_trace_f64(&call->output_hash, value);
}

void tracehash_output_struct_field_bytes(TraceHashCall *call, const char *field, const void *ptr, size_t len) {
  if (!call->active) return;
  hash_str(&call->output_hash, field);
  hash_trace_bytes(&call->output_hash, ptr, len);
}

/* -------------------------------------------------------------------------
 * Public `_as` helpers. The hash updates mirror their positional siblings
 * exactly — the field name is NOT hashed — so these are interchangeable at
 * the TSV level. The name flows into the dclog entry only.
 * ----------------------------------------------------------------------- */

void tracehash_input_u64_as(TraceHashCall *call, const char *name, uint64_t value) {
  if (!call->active) return;
  hash_trace_u64(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IU64=%lu", (unsigned long)value);
  deep_capture_in_u64(call, name, value);
}

void tracehash_input_i64_as(TraceHashCall *call, const char *name, int64_t value) {
  if (!call->active) return;
  hash_trace_u64(&call->input_hash, (uint64_t)value);
  call->input_len++;
  tracehash_append_value(call, "IU64=%lu", (unsigned long)(uint64_t)value);
  deep_capture_in_i64(call, name, value);
}

void tracehash_input_bool_as(TraceHashCall *call, const char *name, int value) {
  if (!call->active) return;
  hash_trace_bool(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IBOOL=%d", value ? 1 : 0);
  deep_capture_in_bool(call, name, value);
}

void tracehash_input_f32_as(TraceHashCall *call, const char *name, float value) {
  union { float f; uint32_t u; } bits;
  if (!call->active) return;
  hash_trace_f32(&call->input_hash, value);
  call->input_len++;
  bits.f = value;
  tracehash_append_value(call, "IF32=%08x/%.9e", bits.u, value);
  deep_capture_in_f32(call, name, value);
}

void tracehash_input_f64_as(TraceHashCall *call, const char *name, double value) {
  if (!call->active) return;
  hash_trace_f64(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IF64=%.17e", value);
  deep_capture_in_f64(call, name, value);
}

void tracehash_input_bytes_as(TraceHashCall *call, const char *name, const void *ptr, size_t len) {
  if (!call->active) return;
  hash_trace_bytes(&call->input_hash, ptr, len);
  call->input_len += (uint64_t)len;
  {
    uint64_t bytes_hash = FNV_OFFSET;
    hash_bytes(&bytes_hash, (const uint8_t *)ptr, len);
    tracehash_append_value(call, "IBYTES=%lu:%016lx", (unsigned long)len, (unsigned long)bytes_hash);
  }
  deep_capture_in_bytes(call, name, ptr, len);
}

void tracehash_output_u64_as(TraceHashCall *call, const char *name, uint64_t value) {
  if (!call->active) return;
  hash_trace_u64(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OU64=%lu", (unsigned long)value);
  deep_capture_out_u64(call, name, value);
}

void tracehash_output_i64_as(TraceHashCall *call, const char *name, int64_t value) {
  if (!call->active) return;
  hash_trace_u64(&call->output_hash, (uint64_t)value);
  call->output_len++;
  tracehash_append_value(call, "OU64=%lu", (unsigned long)(uint64_t)value);
  deep_capture_out_i64(call, name, value);
}

void tracehash_output_bool_as(TraceHashCall *call, const char *name, int value) {
  if (!call->active) return;
  hash_trace_bool(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OBOOL=%d", value ? 1 : 0);
  deep_capture_out_bool(call, name, value);
}

void tracehash_output_f32_as(TraceHashCall *call, const char *name, float value) {
  union { float f; uint32_t u; } bits;
  if (!call->active) return;
  hash_trace_f32(&call->output_hash, value);
  call->output_len++;
  bits.f = value;
  tracehash_append_value(call, "OF32=%08x/%.9e", bits.u, value);
  deep_capture_out_f32(call, name, value);
}

void tracehash_output_f64_as(TraceHashCall *call, const char *name, double value) {
  if (!call->active) return;
  hash_trace_f64(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OF64=%.17e", value);
  deep_capture_out_f64(call, name, value);
}

void tracehash_output_bytes_as(TraceHashCall *call, const char *name, const void *ptr, size_t len) {
  if (!call->active) return;
  hash_trace_bytes(&call->output_hash, ptr, len);
  call->output_len += (uint64_t)len;
  {
    uint64_t bytes_hash = FNV_OFFSET;
    hash_bytes(&bytes_hash, (const uint8_t *)ptr, len);
    tracehash_append_value(call, "OBYTES=%lu:%016lx", (unsigned long)len, (unsigned long)bytes_hash);
  }
  deep_capture_out_bytes(call, name, ptr, len);
}

/* -------------------------------------------------------------------------
 * Per-function dclog file registry and writer.
 * ----------------------------------------------------------------------- */

static void sanitize_function(const char *in, char *out, size_t cap) {
  size_t i;
  size_t n = strlen(in);
  if (n + 1 > cap) n = cap - 1;
  for (i = 0; i < n; i++) {
    char c = in[i];
    int keep = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
               (c >= '0' && c <= '9') || c == '_' || c == '-' || c == '.' || c == ':';
    out[i] = keep ? c : '_';
  }
  out[n] = '\0';
}

/* JSON-escape `in` into `out`. Only handles the characters we care about
 * (backslash and double-quote); control chars and UTF-8 passthrough. */
static void json_escape(const char *in, char *out, size_t cap) {
  size_t o = 0;
  size_t i;
  size_t n = strlen(in);
  for (i = 0; i < n && o + 2 < cap; i++) {
    unsigned char c = (unsigned char)in[i];
    if (c == '"' || c == '\\') {
      if (o + 3 >= cap) break;
      out[o++] = '\\';
      out[o++] = (char)c;
    } else {
      out[o++] = (char)c;
    }
  }
  out[o] = '\0';
}

static TraceHashDeepFile *deep_find_or_create(const char *function) {
  TraceHashDeepFile *node;
  char sanitized[256];
  char escaped[256];
  char path[512];
  char header[1024];
  uint32_t header_len;
  FILE *f;
  time_t now;

  pthread_mutex_lock(&tracehash_deep_registry_mutex);
  for (node = tracehash_deep_files; node != NULL; node = node->next) {
    if (strcmp(node->function, function) == 0) {
      pthread_mutex_unlock(&tracehash_deep_registry_mutex);
      return node;
    }
  }

  sanitize_function(function, sanitized, sizeof(sanitized));
  snprintf(path, sizeof(path), "%s/%s.dclog", tracehash_deep_dir, sanitized);
  f = fopen(path, "wb");
  if (f == NULL) {
    pthread_mutex_unlock(&tracehash_deep_registry_mutex);
    return NULL;
  }

  /* Magic + spec version + header JSON (length-prefixed). */
  fwrite("DCLG", 1, 4, f);
  {
    uint32_t v = 1;
    uint8_t bytes[4];
    bytes[0] = (uint8_t)(v); bytes[1] = (uint8_t)(v >> 8);
    bytes[2] = (uint8_t)(v >> 16); bytes[3] = (uint8_t)(v >> 24);
    fwrite(bytes, 1, 4, f);
  }

  json_escape(function, escaped, sizeof(escaped));
  now = time(NULL);
  {
    const char *mode_str = tracehash_deep_mode_all ? "all" : "first";
    int n;
    if (tracehash_deep_mode_all) {
      n = snprintf(header, sizeof(header),
                   "{\"spec_version\":1,\"source_lang\":\"c\","
                   "\"function_name\":\"%s\",\"function_display\":\"%s\","
                   "\"signature_fingerprint\":\"\",\"timestamp\":%ld,"
                   "\"recorder_config\":{\"mode\":\"all\",\"seed\":0,\"extra\":null},"
                   "\"schemas\":[]}",
                   escaped, escaped, (long)now);
    } else {
      n = snprintf(header, sizeof(header),
                   "{\"spec_version\":1,\"source_lang\":\"c\","
                   "\"function_name\":\"%s\",\"function_display\":\"%s\","
                   "\"signature_fingerprint\":\"\",\"timestamp\":%ld,"
                   "\"recorder_config\":{\"mode\":\"first:%lu\",\"seed\":0,\"extra\":null},"
                   "\"schemas\":[]}",
                   escaped, escaped, (long)now, (unsigned long)tracehash_deep_first_n);
    }
    (void)mode_str;
    if (n < 0 || (size_t)n >= sizeof(header)) {
      fclose(f);
      pthread_mutex_unlock(&tracehash_deep_registry_mutex);
      return NULL;
    }
    header_len = (uint32_t)n;
    {
      uint8_t bytes[4];
      bytes[0] = (uint8_t)(header_len); bytes[1] = (uint8_t)(header_len >> 8);
      bytes[2] = (uint8_t)(header_len >> 16); bytes[3] = (uint8_t)(header_len >> 24);
      fwrite(bytes, 1, 4, f);
    }
    fwrite(header, 1, header_len, f);
  }

  node = (TraceHashDeepFile *)malloc(sizeof(TraceHashDeepFile));
  if (node == NULL) {
    fclose(f);
    pthread_mutex_unlock(&tracehash_deep_registry_mutex);
    return NULL;
  }
  {
    size_t fn_len = strlen(function);
    node->function = (char *)malloc(fn_len + 1);
    if (node->function == NULL) {
      free(node);
      fclose(f);
      pthread_mutex_unlock(&tracehash_deep_registry_mutex);
      return NULL;
    }
    memcpy(node->function, function, fn_len + 1);
  }
  node->file = f;
  pthread_mutex_init(&node->mutex, NULL);
  node->seq = 0;
  node->next = tracehash_deep_files;
  tracehash_deep_files = node;

  pthread_mutex_unlock(&tracehash_deep_registry_mutex);
  return node;
}

/* Content hash over canonical_input_bytes: 0-byte receiver flag + u32 count
 * + in_buf. Matches `src/spec/wire.rs::canonical_input_bytes`. */
static uint64_t deep_content_hash(TraceHashCall *call) {
  uint64_t h = FNV_OFFSET;
  uint8_t count_bytes[4];
  int i;
  hash_u8(&h, 0);
  count_bytes[0] = (uint8_t)(call->deep_in_count);
  count_bytes[1] = (uint8_t)(call->deep_in_count >> 8);
  count_bytes[2] = (uint8_t)(call->deep_in_count >> 16);
  count_bytes[3] = (uint8_t)(call->deep_in_count >> 24);
  for (i = 0; i < 4; i++) hash_u8(&h, count_bytes[i]);
  hash_bytes(&h, call->deep_in_buf, call->deep_in_len);
  return h;
}

/* Assemble and write a framed dclog entry. Returns the seq assigned, or
 * -1 on failure. */
static long deep_write_entry(TraceHashDeepFile *df, TraceHashCall *call) {
  uint8_t *body = NULL;
  size_t body_len = 0;
  size_t body_cap = 0;
  uint64_t content_hash;
  uint32_t assigned_seq;
  uint8_t len_bytes[4];

  assigned_seq = df->seq;

  /* seq u32 */
  deep_buf_u32le(call, &body, &body_len, &body_cap, assigned_seq);
  /* flags u8 (no receiver) */
  deep_buf_u8(call, &body, &body_len, &body_cap, 0);
  /* inputs: count u32 + in_buf */
  deep_buf_u32le(call, &body, &body_len, &body_cap, call->deep_in_count);
  deep_buf_bytes(call, &body, &body_len, &body_cap, call->deep_in_buf, call->deep_in_len);
  /* outcome tag 0 (Return) + outputs count + out_buf */
  deep_buf_u8(call, &body, &body_len, &body_cap, 0);
  deep_buf_u32le(call, &body, &body_len, &body_cap, call->deep_out_count);
  deep_buf_bytes(call, &body, &body_len, &body_cap, call->deep_out_buf, call->deep_out_len);
  /* content_hash u64 */
  content_hash = deep_content_hash(call);
  deep_buf_u64le(call, &body, &body_len, &body_cap, content_hash);

  if (!call->deep_active) {
    /* A reserve failure above flipped deep_active off. */
    free(body);
    return -1;
  }

  len_bytes[0] = (uint8_t)(body_len);
  len_bytes[1] = (uint8_t)(body_len >> 8);
  len_bytes[2] = (uint8_t)(body_len >> 16);
  len_bytes[3] = (uint8_t)(body_len >> 24);

  pthread_mutex_lock(&df->mutex);
  fwrite(len_bytes, 1, 4, df->file);
  fwrite(body, 1, body_len, df->file);
  df->seq++;
  pthread_mutex_unlock(&df->mutex);

  free(body);
  return (long)assigned_seq;
}

void tracehash_deep_flush_all(void) {
  TraceHashDeepFile *node;
  pthread_mutex_lock(&tracehash_deep_registry_mutex);
  for (node = tracehash_deep_files; node != NULL; node = node->next) {
    pthread_mutex_lock(&node->mutex);
    fflush(node->file);
    pthread_mutex_unlock(&node->mutex);
  }
  pthread_mutex_unlock(&tracehash_deep_registry_mutex);
}

void tracehash_finish(TraceHashCall *call) {
  uint64_t elapsed = now_ns() - call->start_ns;
  uint64_t seq;
  long deep_seq_value = -1;
  char deep_seq_field[24];

  if (!call->active) return;

  /* Write the dclog entry first so its seq can be referenced from the
   * TSV row. Respect the sampling mode (first:N) — only emit when the
   * per-function counter is still under the limit. */
  if (call->deep_active && tracehash_deep_dir != NULL) {
    TraceHashDeepFile *df = deep_find_or_create(call->function);
    if (df != NULL) {
      int emit;
      pthread_mutex_lock(&df->mutex);
      emit = tracehash_deep_mode_all || (uint64_t)df->seq < tracehash_deep_first_n;
      pthread_mutex_unlock(&df->mutex);
      if (emit) {
        deep_seq_value = deep_write_entry(df, call);
      }
    }
  }

  if (deep_seq_value >= 0) {
    snprintf(deep_seq_field, sizeof(deep_seq_field), "%ld", deep_seq_value);
  } else {
    deep_seq_field[0] = '-';
    deep_seq_field[1] = '\0';
  }

  if (tracehash_file != NULL) {
    pthread_mutex_lock(&tracehash_mutex);
    seq = tracehash_seq++;
    fprintf(tracehash_file,
            "%s\t%s\t%lu\t%lu\t%s\t%016lx\t%016lx\t%lu\t%lu\t%lu\t%s\t%d\t%s",
            tracehash_run_id,
            tracehash_side,
            (unsigned long)pthread_self(),
            (unsigned long)seq,
            call->function,
            (unsigned long)call->input_hash,
            (unsigned long)call->output_hash,
            (unsigned long)call->input_len,
            (unsigned long)call->output_len,
            (unsigned long)elapsed,
            call->file,
            call->line,
            deep_seq_field);
    if (call->values_enabled) fprintf(tracehash_file, "\t%s", call->values != NULL ? call->values : "");
    fputc('\n', tracehash_file);
    fflush(tracehash_file);
    pthread_mutex_unlock(&tracehash_mutex);
  }

  free(call->values);
  free(call->deep_in_buf);
  free(call->deep_out_buf);
}
