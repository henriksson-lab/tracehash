#ifndef TRACEHASH_C_H
#define TRACEHASH_C_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct TraceHashCall {
  const char *function;
  const char *file;
  int line;
  uint64_t input_hash;
  uint64_t output_hash;
  uint64_t input_len;
  uint64_t output_len;
  uint64_t start_ns;
  char *values;
  size_t values_len;
  size_t values_cap;
  int values_enabled;
  int active;
  /* Deep-log capture: byte buffers holding the pre-encoded dclog Value
   * stream for inputs/outputs on this call. Populated by the positional
   * (auto-named) and `_as` (explicitly-named) helpers. Allocated lazily. */
  int deep_active;
  uint8_t *deep_in_buf;
  size_t deep_in_len;
  size_t deep_in_cap;
  uint32_t deep_in_count;
  uint32_t input_counter;
  uint8_t *deep_out_buf;
  size_t deep_out_len;
  size_t deep_out_cap;
  uint32_t deep_out_count;
  uint32_t output_counter;
} TraceHashCall;

TraceHashCall tracehash_begin(const char *function, const char *file, int line);
void tracehash_input_u64(TraceHashCall *call, uint64_t value);
void tracehash_input_i64(TraceHashCall *call, int64_t value);
void tracehash_input_bool(TraceHashCall *call, int value);
void tracehash_input_f32(TraceHashCall *call, float value);
void tracehash_input_f64(TraceHashCall *call, double value);
void tracehash_input_f32_quant(TraceHashCall *call, float value, float quantum);
void tracehash_input_bytes(TraceHashCall *call, const void *ptr, size_t len);
void tracehash_output_u64(TraceHashCall *call, uint64_t value);
void tracehash_output_i64(TraceHashCall *call, int64_t value);
void tracehash_output_bool(TraceHashCall *call, int value);
void tracehash_output_f32(TraceHashCall *call, float value);
void tracehash_output_f64(TraceHashCall *call, double value);
void tracehash_output_f32_quant(TraceHashCall *call, float value, float quantum);
void tracehash_output_bytes(TraceHashCall *call, const void *ptr, size_t len);
void tracehash_input_field_u64(TraceHashCall *call, const char *field, uint64_t value);
void tracehash_input_field_i64(TraceHashCall *call, const char *field, int64_t value);
void tracehash_input_field_bool(TraceHashCall *call, const char *field, int value);
void tracehash_input_field_f32(TraceHashCall *call, const char *field, float value);
void tracehash_input_field_f64(TraceHashCall *call, const char *field, double value);
void tracehash_output_field_u64(TraceHashCall *call, const char *field, uint64_t value);
void tracehash_output_field_i64(TraceHashCall *call, const char *field, int64_t value);
void tracehash_output_field_bool(TraceHashCall *call, const char *field, int value);
void tracehash_output_field_f32(TraceHashCall *call, const char *field, float value);
void tracehash_output_field_f64(TraceHashCall *call, const char *field, double value);
void tracehash_input_struct_begin(TraceHashCall *call, const char *name);
void tracehash_output_struct_begin(TraceHashCall *call, const char *name);
void tracehash_input_struct_field_u64(TraceHashCall *call, const char *field, uint64_t value);
void tracehash_input_struct_field_i64(TraceHashCall *call, const char *field, int64_t value);
void tracehash_input_struct_field_bool(TraceHashCall *call, const char *field, int value);
void tracehash_input_struct_field_f32(TraceHashCall *call, const char *field, float value);
void tracehash_input_struct_field_f64(TraceHashCall *call, const char *field, double value);
void tracehash_input_struct_field_bytes(TraceHashCall *call, const char *field, const void *ptr, size_t len);
void tracehash_output_struct_field_u64(TraceHashCall *call, const char *field, uint64_t value);
void tracehash_output_struct_field_i64(TraceHashCall *call, const char *field, int64_t value);
void tracehash_output_struct_field_bool(TraceHashCall *call, const char *field, int value);
void tracehash_output_struct_field_f32(TraceHashCall *call, const char *field, float value);
void tracehash_output_struct_field_f64(TraceHashCall *call, const char *field, double value);
void tracehash_output_struct_field_bytes(TraceHashCall *call, const char *field, const void *ptr, size_t len);
void tracehash_finish(TraceHashCall *call);

/* Named-field (`_as`) helpers: produce the same hash bytes as the positional
 * helpers (the name does NOT enter the FNV hash stream), but attach `name`
 * to the matching dclog entry in deep-mode capture. */
void tracehash_input_u64_as   (TraceHashCall *call, const char *name, uint64_t value);
void tracehash_input_i64_as   (TraceHashCall *call, const char *name, int64_t value);
void tracehash_input_bool_as  (TraceHashCall *call, const char *name, int value);
void tracehash_input_f32_as   (TraceHashCall *call, const char *name, float value);
void tracehash_input_f64_as   (TraceHashCall *call, const char *name, double value);
void tracehash_input_bytes_as (TraceHashCall *call, const char *name, const void *ptr, size_t len);
void tracehash_output_u64_as  (TraceHashCall *call, const char *name, uint64_t value);
void tracehash_output_i64_as  (TraceHashCall *call, const char *name, int64_t value);
void tracehash_output_bool_as (TraceHashCall *call, const char *name, int value);
void tracehash_output_f32_as  (TraceHashCall *call, const char *name, float value);
void tracehash_output_f64_as  (TraceHashCall *call, const char *name, double value);
void tracehash_output_bytes_as(TraceHashCall *call, const char *name, const void *ptr, size_t len);

/* Flush any buffered "last" dclog entries and close per-function writers.
 * Call before process exit when using `firstlast` or `prob` with keep-last.
 * atexit() is not installed automatically — the Rust side also requires an
 * explicit call, so this keeps the semantics symmetric. */
void tracehash_deep_flush_all(void);

#ifdef __cplusplus
}
#endif

#define TH_CALL(name) TraceHashCall th_call = tracehash_begin((name), __FILE__, __LINE__)
#define TH_CALL_N(var, name) TraceHashCall var = tracehash_begin((name), __FILE__, __LINE__)
#define TH_IN_U64_TO(call, value) tracehash_input_u64((call), (uint64_t)(value))
#define TH_IN_I64_TO(call, value) tracehash_input_i64((call), (int64_t)(value))
#define TH_IN_BOOL_TO(call, value) tracehash_input_bool((call), (value))
#define TH_IN_F32_TO(call, value) tracehash_input_f32((call), (float)(value))
#define TH_IN_F64_TO(call, value) tracehash_input_f64((call), (double)(value))
#define TH_IN_F32_Q_TO(call, value, quantum) tracehash_input_f32_quant((call), (float)(value), (float)(quantum))
#define TH_IN_BYTES_TO(call, ptr, len) tracehash_input_bytes((call), (ptr), (len))
#define TH_OUT_U64_TO(call, value) tracehash_output_u64((call), (uint64_t)(value))
#define TH_OUT_I64_TO(call, value) tracehash_output_i64((call), (int64_t)(value))
#define TH_OUT_BOOL_TO(call, value) tracehash_output_bool((call), (value))
#define TH_OUT_F32_TO(call, value) tracehash_output_f32((call), (float)(value))
#define TH_OUT_F64_TO(call, value) tracehash_output_f64((call), (double)(value))
#define TH_OUT_F32_Q_TO(call, value, quantum) tracehash_output_f32_quant((call), (float)(value), (float)(quantum))
#define TH_OUT_BYTES_TO(call, ptr, len) tracehash_output_bytes((call), (ptr), (len))
#define TH_IN_FIELD_U64_TO(call, field, value) tracehash_input_field_u64((call), (field), (uint64_t)(value))
#define TH_IN_FIELD_I64_TO(call, field, value) tracehash_input_field_i64((call), (field), (int64_t)(value))
#define TH_IN_FIELD_BOOL_TO(call, field, value) tracehash_input_field_bool((call), (field), (value))
#define TH_IN_FIELD_F32_TO(call, field, value) tracehash_input_field_f32((call), (field), (float)(value))
#define TH_IN_FIELD_F64_TO(call, field, value) tracehash_input_field_f64((call), (field), (double)(value))
#define TH_OUT_FIELD_U64_TO(call, field, value) tracehash_output_field_u64((call), (field), (uint64_t)(value))
#define TH_OUT_FIELD_I64_TO(call, field, value) tracehash_output_field_i64((call), (field), (int64_t)(value))
#define TH_OUT_FIELD_BOOL_TO(call, field, value) tracehash_output_field_bool((call), (field), (value))
#define TH_OUT_FIELD_F32_TO(call, field, value) tracehash_output_field_f32((call), (field), (float)(value))
#define TH_OUT_FIELD_F64_TO(call, field, value) tracehash_output_field_f64((call), (field), (double)(value))
#define TH_FINISH_TO(call) tracehash_finish((call))

#define TH_IN_U64(value) TH_IN_U64_TO(&th_call, value)
#define TH_IN_I64(value) TH_IN_I64_TO(&th_call, value)
#define TH_IN_BOOL(value) TH_IN_BOOL_TO(&th_call, value)
#define TH_IN_F32(value) TH_IN_F32_TO(&th_call, value)
#define TH_IN_F64(value) TH_IN_F64_TO(&th_call, value)
#define TH_IN_F32_Q(value, quantum) TH_IN_F32_Q_TO(&th_call, value, quantum)
#define TH_IN_BYTES(ptr, len) TH_IN_BYTES_TO(&th_call, ptr, len)
#define TH_OUT_U64(value) TH_OUT_U64_TO(&th_call, value)
#define TH_OUT_I64(value) TH_OUT_I64_TO(&th_call, value)
#define TH_OUT_BOOL(value) TH_OUT_BOOL_TO(&th_call, value)
#define TH_OUT_F32(value) TH_OUT_F32_TO(&th_call, value)
#define TH_OUT_F64(value) TH_OUT_F64_TO(&th_call, value)
#define TH_OUT_F32_Q(value, quantum) TH_OUT_F32_Q_TO(&th_call, value, quantum)
#define TH_OUT_BYTES(ptr, len) TH_OUT_BYTES_TO(&th_call, ptr, len)
#define TH_IN_FIELD_U64(field, value) TH_IN_FIELD_U64_TO(&th_call, field, value)
#define TH_IN_FIELD_I64(field, value) TH_IN_FIELD_I64_TO(&th_call, field, value)
#define TH_IN_FIELD_BOOL(field, value) TH_IN_FIELD_BOOL_TO(&th_call, field, value)
#define TH_IN_FIELD_F32(field, value) TH_IN_FIELD_F32_TO(&th_call, field, value)
#define TH_IN_FIELD_F64(field, value) TH_IN_FIELD_F64_TO(&th_call, field, value)
#define TH_OUT_FIELD_U64(field, value) TH_OUT_FIELD_U64_TO(&th_call, field, value)
#define TH_OUT_FIELD_I64(field, value) TH_OUT_FIELD_I64_TO(&th_call, field, value)
#define TH_OUT_FIELD_BOOL(field, value) TH_OUT_FIELD_BOOL_TO(&th_call, field, value)
#define TH_OUT_FIELD_F32(field, value) TH_OUT_FIELD_F32_TO(&th_call, field, value)
#define TH_OUT_FIELD_F64(field, value) TH_OUT_FIELD_F64_TO(&th_call, field, value)
#define TH_FINISH() TH_FINISH_TO(&th_call)

#define TH_IN_U64_AS_TO(call, name, value)   tracehash_input_u64_as((call), (name), (uint64_t)(value))
#define TH_IN_I64_AS_TO(call, name, value)   tracehash_input_i64_as((call), (name), (int64_t)(value))
#define TH_IN_BOOL_AS_TO(call, name, value)  tracehash_input_bool_as((call), (name), (value))
#define TH_IN_F32_AS_TO(call, name, value)   tracehash_input_f32_as((call), (name), (float)(value))
#define TH_IN_F64_AS_TO(call, name, value)   tracehash_input_f64_as((call), (name), (double)(value))
#define TH_IN_BYTES_AS_TO(call, name, ptr, len) tracehash_input_bytes_as((call), (name), (ptr), (len))
#define TH_OUT_U64_AS_TO(call, name, value)  tracehash_output_u64_as((call), (name), (uint64_t)(value))
#define TH_OUT_I64_AS_TO(call, name, value)  tracehash_output_i64_as((call), (name), (int64_t)(value))
#define TH_OUT_BOOL_AS_TO(call, name, value) tracehash_output_bool_as((call), (name), (value))
#define TH_OUT_F32_AS_TO(call, name, value)  tracehash_output_f32_as((call), (name), (float)(value))
#define TH_OUT_F64_AS_TO(call, name, value)  tracehash_output_f64_as((call), (name), (double)(value))
#define TH_OUT_BYTES_AS_TO(call, name, ptr, len) tracehash_output_bytes_as((call), (name), (ptr), (len))

#define TH_IN_U64_AS(name, value)    TH_IN_U64_AS_TO(&th_call, name, value)
#define TH_IN_I64_AS(name, value)    TH_IN_I64_AS_TO(&th_call, name, value)
#define TH_IN_BOOL_AS(name, value)   TH_IN_BOOL_AS_TO(&th_call, name, value)
#define TH_IN_F32_AS(name, value)    TH_IN_F32_AS_TO(&th_call, name, value)
#define TH_IN_F64_AS(name, value)    TH_IN_F64_AS_TO(&th_call, name, value)
#define TH_IN_BYTES_AS(name, ptr, len) TH_IN_BYTES_AS_TO(&th_call, name, ptr, len)
#define TH_OUT_U64_AS(name, value)   TH_OUT_U64_AS_TO(&th_call, name, value)
#define TH_OUT_I64_AS(name, value)   TH_OUT_I64_AS_TO(&th_call, name, value)
#define TH_OUT_BOOL_AS(name, value)  TH_OUT_BOOL_AS_TO(&th_call, name, value)
#define TH_OUT_F32_AS(name, value)   TH_OUT_F32_AS_TO(&th_call, name, value)
#define TH_OUT_F64_AS(name, value)   TH_OUT_F64_AS_TO(&th_call, name, value)
#define TH_OUT_BYTES_AS(name, ptr, len) TH_OUT_BYTES_AS_TO(&th_call, name, ptr, len)

#define TH_FIELD_IN_U64(call, value, field) tracehash_input_struct_field_u64((call), #field, (uint64_t)((value)->field));
#define TH_FIELD_IN_I64(call, value, field) tracehash_input_struct_field_i64((call), #field, (int64_t)((value)->field));
#define TH_FIELD_IN_BOOL(call, value, field) tracehash_input_struct_field_bool((call), #field, ((value)->field));
#define TH_FIELD_IN_F32(call, value, field) tracehash_input_struct_field_f32((call), #field, (float)((value)->field));
#define TH_FIELD_IN_F64(call, value, field) tracehash_input_struct_field_f64((call), #field, (double)((value)->field));
#define TH_FIELD_IN_BYTES(call, value, field, len) tracehash_input_struct_field_bytes((call), #field, (value)->field, (len));
#define TH_FIELD_OUT_U64(call, value, field) tracehash_output_struct_field_u64((call), #field, (uint64_t)((value)->field));
#define TH_FIELD_OUT_I64(call, value, field) tracehash_output_struct_field_i64((call), #field, (int64_t)((value)->field));
#define TH_FIELD_OUT_BOOL(call, value, field) tracehash_output_struct_field_bool((call), #field, ((value)->field));
#define TH_FIELD_OUT_F32(call, value, field) tracehash_output_struct_field_f32((call), #field, (float)((value)->field));
#define TH_FIELD_OUT_F64(call, value, field) tracehash_output_struct_field_f64((call), #field, (double)((value)->field));
#define TH_FIELD_OUT_BYTES(call, value, field, len) tracehash_output_struct_field_bytes((call), #field, (value)->field, (len));

#define TH_DEFINE_STRUCT_HASH(Type, FIELDS) \
  static void tracehash_input_struct_##Type(TraceHashCall *call, const Type *value) { \
    tracehash_input_struct_begin((call), #Type); \
    FIELDS(TH_FIELD_IN, call, value) \
  } \
  static void tracehash_output_struct_##Type(TraceHashCall *call, const Type *value) { \
    tracehash_output_struct_begin((call), #Type); \
    FIELDS(TH_FIELD_OUT, call, value) \
  }

#define TH_IN_STRUCT_TO(call, Type, value) tracehash_input_struct_##Type((call), (value))
#define TH_OUT_STRUCT_TO(call, Type, value) tracehash_output_struct_##Type((call), (value))
#define TH_IN_STRUCT(Type, value) TH_IN_STRUCT_TO(&th_call, Type, value)
#define TH_OUT_STRUCT(Type, value) TH_OUT_STRUCT_TO(&th_call, Type, value)

#endif
