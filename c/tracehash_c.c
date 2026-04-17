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
  if (tracehash_initialized) return;
  tracehash_initialized = 1;
  out = getenv("TRACEHASH_OUT");
  if (out == NULL || out[0] == '\0') return;
  tracehash_file = fopen(out, "w");
  if (getenv("TRACEHASH_SIDE") != NULL) tracehash_side = getenv("TRACEHASH_SIDE");
  if (getenv("TRACEHASH_RUN_ID") != NULL) tracehash_run_id = getenv("TRACEHASH_RUN_ID");
  if (getenv("TRACEHASH_VALUES") != NULL &&
      getenv("TRACEHASH_VALUES")[0] != '\0' &&
      strcmp(getenv("TRACEHASH_VALUES"), "0") != 0)
    tracehash_values = 1;
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
  call.active = tracehash_file != NULL;
  if (call.active) {
    hash_str(&call.input_hash, function);
    hash_str(&call.output_hash, function);
  }
  return call;
}

void tracehash_input_u64(TraceHashCall *call, uint64_t value) {
  if (!call->active) return;
  hash_trace_u64(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IU64=%lu", (unsigned long)value);
}

void tracehash_input_i64(TraceHashCall *call, int64_t value) {
  tracehash_input_u64(call, (uint64_t)value);
}

void tracehash_input_bool(TraceHashCall *call, int value) {
  if (!call->active) return;
  hash_trace_bool(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IBOOL=%d", value ? 1 : 0);
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
}

void tracehash_input_f64(TraceHashCall *call, double value) {
  if (!call->active) return;
  hash_trace_f64(&call->input_hash, value);
  call->input_len++;
  tracehash_append_value(call, "IF64=%.17e", value);
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
}

void tracehash_output_u64(TraceHashCall *call, uint64_t value) {
  if (!call->active) return;
  hash_trace_u64(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OU64=%lu", (unsigned long)value);
}

void tracehash_output_i64(TraceHashCall *call, int64_t value) {
  tracehash_output_u64(call, (uint64_t)value);
}

void tracehash_output_bool(TraceHashCall *call, int value) {
  if (!call->active) return;
  hash_trace_bool(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OBOOL=%d", value ? 1 : 0);
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
}

void tracehash_output_f64(TraceHashCall *call, double value) {
  if (!call->active) return;
  hash_trace_f64(&call->output_hash, value);
  call->output_len++;
  tracehash_append_value(call, "OF64=%.17e", value);
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

void tracehash_finish(TraceHashCall *call) {
  uint64_t elapsed = now_ns() - call->start_ns;
  uint64_t seq;
  if (!call->active) return;
  pthread_mutex_lock(&tracehash_mutex);
  seq = tracehash_seq++;
  fprintf(tracehash_file,
          "%s\t%s\t%lu\t%lu\t%s\t%016lx\t%016lx\t%lu\t%lu\t%lu\t%s\t%d",
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
          call->line);
  if (call->values_enabled) fprintf(tracehash_file, "\t%s", call->values != NULL ? call->values : "");
  fputc('\n', tracehash_file);
  fflush(tracehash_file);
  pthread_mutex_unlock(&tracehash_mutex);
  free(call->values);
}
