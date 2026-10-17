// Native thread stack bounds, independent of the JavaScript engine.
#include <cstdint>
#if defined(__APPLE__) || defined(__linux__)
#include <pthread.h>
#elif defined(_WIN32)
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0602
#endif
#include <windows.h>
#endif

extern "C" bool light_cljs_thread_stack_bounds(uintptr_t* low,
                                               uintptr_t* high) {
#if defined(__APPLE__)
  auto thread = pthread_self();
  *high = reinterpret_cast<uintptr_t>(pthread_get_stackaddr_np(thread));
  size_t size = pthread_get_stacksize_np(thread);
  if (size > *high) return false;
  *low = *high - size;
#elif defined(__linux__)
  pthread_attr_t attributes;
  if (pthread_getattr_np(pthread_self(), &attributes) != 0) return false;
  void* address = nullptr;
  size_t size = 0;
  int result = pthread_attr_getstack(&attributes, &address, &size);
  pthread_attr_destroy(&attributes);
  if (result != 0) return false;
  *low = reinterpret_cast<uintptr_t>(address);
  if (size > UINTPTR_MAX - *low) return false;
  *high = *low + size;
#elif defined(_WIN32)
  ULONG_PTR committed_low, stack_high;
  GetCurrentThreadStackLimits(&committed_low, &stack_high);
  // Query the reservation base explicitly, without relying on undocumented TEB
  // offsets. The system stack bounds do not measure physical committed bytes.
  MEMORY_BASIC_INFORMATION memory;
  if (VirtualQuery(&memory, &memory, sizeof(memory)) != sizeof(memory)) return false;
  *low = reinterpret_cast<uintptr_t>(memory.AllocationBase);
  *high = stack_high;
#else
  return false;
#endif
  return *low != 0 && *low < *high;
}
