#include <cstdint>
#include <cstdio>
#if defined(_WIN32)
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0602
#endif
#include <windows.h>
#else
#include <pthread.h>
#endif

extern "C" bool light_cljs_thread_stack_bounds(uintptr_t*, uintptr_t*);

__attribute__((noinline)) bool descend(unsigned depth, uintptr_t low, uintptr_t high) {
  volatile unsigned char padding[4096];
  padding[0] = static_cast<unsigned char>(depth);
  padding[4095] = 23;
  uintptr_t current = reinterpret_cast<uintptr_t>(&padding[0]);
  uintptr_t actual_low = 0, actual_high = 0;
  if (!light_cljs_thread_stack_bounds(&actual_low, &actual_high) ||
      low != actual_low || high != actual_high || current <= low || current >= high)
    return false;
  bool result = depth == 0 || descend(depth - 1, low, high);
  return result && padding[0] == static_cast<unsigned char>(depth) && padding[4095] == 23;
}

bool probe(const char* name) {
  uintptr_t low = 0, high = 0;
  uintptr_t current = reinterpret_cast<uintptr_t>(&low);
  if (!light_cljs_thread_stack_bounds(&low, &high) || !(low < current && current < high)) return false;
  std::printf("%s: reservation=%llu available=%llu", name,
    static_cast<unsigned long long>(high - low), static_cast<unsigned long long>(current - low));
#if defined(_WIN32)
  ULONG_PTR committed_low, committed_high;
  GetCurrentThreadStackLimits(&committed_low, &committed_high);
  std::printf(" committed=%llu", static_cast<unsigned long long>(committed_high - committed_low));
  if (low > committed_low || high != committed_high) return false;
#endif
  std::puts("");
  return descend(64, low, high);
}

#if defined(_WIN32)
DWORD WINAPI worker(void*) { return probe("one-MiB worker") ? 0 : 1; }
#else
void* worker(void* result) { *static_cast<bool*>(result) = probe("one-MiB worker"); return nullptr; }
#endif

int main() {
  if (!probe("main")) return 1;
#if defined(_WIN32)
  HANDLE thread = CreateThread(nullptr, 1024 * 1024, worker, nullptr, STACK_SIZE_PARAM_IS_A_RESERVATION, nullptr);
  if (!thread) return 2;
  DWORD code = 1;
  bool ok = WaitForSingleObject(thread, INFINITE) == WAIT_OBJECT_0 && GetExitCodeThread(thread, &code);
  CloseHandle(thread);
  if (!ok || code != 0) return 3;
#else
  pthread_attr_t attributes;
  if (pthread_attr_init(&attributes) != 0) return 2;
  if (pthread_attr_setstacksize(&attributes, 1024 * 1024) != 0) return 3;
  pthread_t thread;
  bool ok = false;
  int result = pthread_create(&thread, &attributes, worker, &ok);
  pthread_attr_destroy(&attributes);
  if (result != 0 || pthread_join(thread, nullptr) != 0 || !ok) return 4;
#endif
  std::puts("PASS: exact production shim, stable bounds through 256 KiB recursion and one-MiB worker");
  return 0;
}
