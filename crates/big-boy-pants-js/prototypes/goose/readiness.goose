import std;
namespace readiness;

// Payloads live in their bounded source queues (or atomically published files).
// One token coalesces arbitrarily many publications. Refusal means a token is
// already pending, never that the payload may be discarded. Clear BEFORE
// scanning every source; a final blocking receive is then free of lost wakes.
struct Wake { ready: bool = true }
struct InputCredit { ready: bool = true }
fn notify() -> bool { return qput(Wake {}); }
fn clear() { loop { let wake, present = qpoll<Wake>(); if !present { return; } } }
fn park() { qget<Wake>(); }
