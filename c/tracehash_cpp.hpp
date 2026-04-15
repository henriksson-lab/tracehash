#ifndef TRACEHASH_CPP_HPP
#define TRACEHASH_CPP_HPP

#include "tracehash_c.h"

#include <cstddef>
#include <cstdint>

namespace tracehash {

class Call {
 public:
  Call(const char *function, const char *file, int line)
      : call_(tracehash_begin(function, file, line)), finished_(false) {}

  Call(const Call &) = delete;
  Call &operator=(const Call &) = delete;

  Call(Call &&other) noexcept : call_(other.call_), finished_(other.finished_) {
    other.finished_ = true;
  }

  Call &operator=(Call &&other) noexcept {
    if (this != &other) {
      finish();
      call_ = other.call_;
      finished_ = other.finished_;
      other.finished_ = true;
    }
    return *this;
  }

  ~Call() { finish(); }

  TraceHashCall *raw() { return &call_; }

  void input_u64(std::uint64_t value) { tracehash_input_u64(&call_, value); }
  void input_i64(std::int64_t value) { tracehash_input_i64(&call_, value); }
  void input_bool(bool value) { tracehash_input_bool(&call_, value ? 1 : 0); }
  void input_f32(float value) { tracehash_input_f32(&call_, value); }
  void input_f64(double value) { tracehash_input_f64(&call_, value); }
  void input_bytes(const void *ptr, std::size_t len) { tracehash_input_bytes(&call_, ptr, len); }

  void output_u64(std::uint64_t value) { tracehash_output_u64(&call_, value); }
  void output_i64(std::int64_t value) { tracehash_output_i64(&call_, value); }
  void output_bool(bool value) { tracehash_output_bool(&call_, value ? 1 : 0); }
  void output_f32(float value) { tracehash_output_f32(&call_, value); }
  void output_f64(double value) { tracehash_output_f64(&call_, value); }
  void output_bytes(const void *ptr, std::size_t len) { tracehash_output_bytes(&call_, ptr, len); }

  void finish() {
    if (!finished_) {
      tracehash_finish(&call_);
      finished_ = true;
    }
  }

 private:
  TraceHashCall call_;
  bool finished_;
};

}  // namespace tracehash

#define TRACEHASH_CALL(name) tracehash::Call th_call((name), __FILE__, __LINE__)
#define TRACEHASH_CALL_N(var, name) tracehash::Call var((name), __FILE__, __LINE__)

#endif
