#include <cstdlib>
#include <new>
#if defined(__clang__) || defined(__GNUC__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-parameter"
#pragma GCC diagnostic ignored "-Wcomment"
#endif
#include "v8-array-buffer.h"
#if defined(__clang__) || defined(__GNUC__)
#pragma GCC diagnostic pop
#endif

// Linking an upstream archive with our extended allocator vtable is unsafe.
// Only the patched engine exports this versioned ABI marker.
extern "C" void light_v8_backing_store_budget_abi_v1();

// These callbacks only touch atomic Rust accounting. They never enter V8.
struct LightBufferBudget {
  bool (*reserve)(const void*, size_t);
  void (*release)(const void*, size_t);
  void (*failed)(const void*);
  void (*drop)(const void*);
};

class LightBufferAllocator final : public v8::ArrayBuffer::Allocator {
 public:
  LightBufferAllocator(const void* state, const LightBufferBudget* callbacks)
      : state_(state), callbacks_(callbacks) {}
  ~LightBufferAllocator() override { callbacks_->drop(state_); }

  void* Allocate(size_t length) override {
    if (!callbacks_->reserve(state_, length)) return nullptr;
    void* data = std::calloc(length ? length : 1, 1);
    if (!data) callbacks_->release(state_, length);
    return data;
  }
  void* AllocateUninitialized(size_t length) override { return Allocate(length); }
  void Free(void* data, size_t length) override {
    std::free(data);
    callbacks_->release(state_, length);
  }
  bool HasBackingStoreMemoryBudget() const override { return true; }
  bool ReserveBackingStoreMemory(size_t length) override {
    return callbacks_->reserve(state_, length);
  }
  void ReleaseBackingStoreMemory(size_t length) override {
    callbacks_->release(state_, length);
  }
  void BackingStoreAllocationFailed() override { callbacks_->failed(state_); }

 private:
  const void* state_;
  const LightBufferBudget* callbacks_;
};

extern "C" v8::ArrayBuffer::Allocator* light_v8_buffer_allocator(
    const void* state, const LightBufferBudget* callbacks) {
  light_v8_backing_store_budget_abi_v1();
  return new (std::nothrow) LightBufferAllocator(state, callbacks);
}
