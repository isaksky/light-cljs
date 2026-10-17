// The pinned rusty_v8 bindings expose the initial ResourceConstraints boundary,
// but not Isolate::SetStackLimit for refreshing an outer operation's boundary.
#if defined(__clang__) || defined(__GNUC__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-parameter"
#pragma GCC diagnostic ignored "-Wcomment"
#endif
#include "v8-isolate.h"
#if defined(__clang__) || defined(__GNUC__)
#pragma GCC diagnostic pop
#endif

extern "C" void light_cljs_v8_set_stack_limit(v8::Isolate* isolate,
                                             uintptr_t boundary) {
  isolate->SetStackLimit(boundary);
}

