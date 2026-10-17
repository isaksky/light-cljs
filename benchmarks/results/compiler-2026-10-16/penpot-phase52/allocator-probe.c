#include "mimalloc.h"
#include <assert.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static void* cross_thread_free(void* p) {
  assert(((unsigned char*)p)[4095] == 73);
  mi_free(p);
  for (size_t n = 1; n <= 10000; n++) {
    void* q = mi_malloc(n);
    assert(q);
    memset(q, 91, n);
    mi_free(q);
  }
  return NULL;
}

int main(void) {
  const size_t sizes[] = {0, 1, 7, 16, 31, 128, 4096, 65537, 1048576};
  for (size_t a = 8; a <= 4096; a *= 2) {
    for (size_t i = 0; i < sizeof(sizes) / sizeof(sizes[0]); i++) {
      size_t n = sizes[i];
      unsigned char* p = mi_zalloc_aligned(n, a);
      assert(p && (uintptr_t)p % a == 0);
      for (size_t j = 0; j < n; j++) assert(p[j] == 0);
      memset(p, 42, n);
      p = mi_realloc_aligned(p, n + 113, a);
      assert(p && (uintptr_t)p % a == 0);
      for (size_t j = 0; j < n; j++) assert(p[j] == 42);
      mi_free(p);
    }
  }
  pthread_t threads[4];
  for (size_t i = 0; i < 4; i++) {
    void* p = mi_malloc(4096);
    assert(p);
    memset(p, 73, 4096);
    assert(pthread_create(&threads[i], NULL, cross_thread_free, p) == 0);
  }
  for (size_t i = 0; i < 4; i++) assert(pthread_join(threads[i], NULL) == 0);
  mi_collect(true);
  printf("mimalloc %d: alignment, zeroing, realloc preservation and cross-thread frees passed\n", mi_version());
  return 0;
}
