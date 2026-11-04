import std;
import .readiness;

struct Payload { value: i64 }
struct Release { value: i64 }
struct Ack { value: i64 }
thread_fn producer() {
    for i in 3 {
        let release = qget<Release>();
        assert(qput(Payload { value: release.value }));
        readiness::notify();
        assert(qput(Ack { value: release.value }));
    }
}
fn main() {
    qlimit<readiness::Wake>(1); qlimit<Payload>(1);
    // Before the scan, repeated notices saturate and coalesce safely.
    assert(qput(Payload { value: 1 })); assert(readiness::notify());
    assert(!readiness::notify()); assert(!qput(Payload { value: 999 }));
    readiness::clear(); assert(qget<Payload>().value == 1);
    let thread = thread_spawn(producer);
    // In the transition after clearing, before the source recheck.
    readiness::clear(); assert(qput(Release { value: 2 }));
    assert(qget<Ack>().value == 2); assert(qget<Payload>().value == 2);
    // The redundant token is a valid spurious wake; no duplicate payload.
    readiness::park(); let duplicate, present = qpoll<Payload>(); assert(!present);
    // Publication after the last source recheck, before blocking receive.
    readiness::clear(); let empty, ready = qpoll<Payload>(); assert(!ready);
    assert(qput(Release { value: 3 })); assert(qget<Ack>().value == 3);
    readiness::park(); assert(qget<Payload>().value == 3);
    // Publication races the atomic wait. There is no deadline/polling path.
    readiness::clear(); assert(qput(Release { value: 4 }));
    readiness::park(); assert(qget<Payload>().value == 4); assert(qget<Ack>().value == 4);
    assert(thread_wait(thread));
    // Actual credit protocol: a full payload queue prevents another producer
    // admission, and consuming one payload returns precisely one credit.
    qlimit<readiness::InputCredit>(1); assert(qput(readiness::InputCredit {}));
    qget<readiness::InputCredit>(); assert(qput(Payload { value: 5 }));
    let credit, admitted = qpoll<readiness::InputCredit>(); assert(!admitted);
    assert(qget<Payload>().value == 5); assert(qput(readiness::InputCredit {}));
    qget<readiness::InputCredit>(); readiness::clear();
    print("readiness: coalescing/full-payload/backpressure and 3 transition publications passed; 3 parks, no timed receive");
}
