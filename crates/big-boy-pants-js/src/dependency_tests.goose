import std;
import .dependency;

fn key(definition: i64 = 1, kind: i64 = 0, session: i64 = 7, generation: i64 = 3, compilation: i64 = 2) -> dependency::Key { return dependency::Key { compilation: compilation, session: session, generation: generation, definition: definition, kind: kind }; }
fn completion(k: dependency::Key, token: i64, outcome: dependency::Outcome = dependency::Outcome.Success, payload: i64 = 41) -> dependency::Completion { return dependency::Completion { key: k, producer: token, terminal: dependency::Terminal { outcome: outcome, payload: payload } }; }
fn main() {
    // Adapted ordered equivalents of be0e53e7:tests/dependency.rs and
    // 27d517f4:src/dep.rs (including its two validated completion regression).
    for outcome in [dependency::Outcome.Success, dependency::Outcome.Failed, dependency::Outcome.Cancelled] {
        var table = dependency::Table { compilation: 2, session: 7 };
        let first = dependency::demand(table, 10, key());
        assert(first.cell == 0 && first.producer > 0 && first.terminal.outcome == dependency::Outcome.Pending);
        let duplicate = dependency::demand(table, 10, key());
        let second = dependency::demand(table, 11, key());
        assert(duplicate.producer == 0 && second.producer == 0 && table.requests.len == 1 && table.edges.len == 2);
        assert(!dependency::install(table, completion(key(), first.producer + 1)) && table.deliveries.len == 0);
        for wrong in [key(1, 1), key(1, 0, 8), key(1, 0, 7, 4), key(1, 0, 7, 3, 3)] { assert(!dependency::install(table, completion(wrong, first.producer))); }
        let a = completion(key(), first.producer, outcome); let b = completion(key(), first.producer, dependency::Outcome.Success, 99);
        assert(dependency::may_complete(table, a) && dependency::may_complete(table, b));
        assert(dependency::install(table, a) && !dependency::install(table, b));
        assert(table.cells[0].terminal.outcome == outcome && table.cells[0].terminal.payload == 41 && table.deliveries.len == 2);
        assert(table.deliveries[0].consumer == 10 && table.deliveries[1].consumer == 11);
        let ready = dependency::demand(table, 12, key()); assert(ready.producer == 0 && ready.terminal.outcome == outcome);
        dependency::demand(table, 12, key()); dependency::demand(table, 10, key());
        assert(table.deliveries.len == 3 && table.edges.len == 3 && table.edges[2].delivered);
        assert(!dependency::install(table, completion(key(), first.producer, dependency::Outcome.Cancelled)));
    }
    {
        var table = dependency::Table { compilation: 2, session: 7 };
        let claim = dependency::ensure(table, key()); assert(claim.producer > 0);
        assert(dependency::install(table, completion(key(), claim.producer)));
        let read = dependency::demand(table, 1, key()); assert(read.terminal.payload == 41 && table.edges.len == 1 && table.deliveries.len == 1);
        // Initializer, callable, query/version, session and compilation identity.
        let callable = dependency::demand(table, 1, key(1, 1)); let version = dependency::demand(table, 1, key(1, 0, 7, 4));
        assert(callable.producer > 0 && version.producer > 0 && table.cells.len == 3);
        assert(dependency::demand(table, 1, key(1, 0, 8)).cell == -1 && dependency::demand(table, 1, key(1, 0, 7, 3, 3)).cell == -1);
    }
    {
        var table = dependency::Table { compilation: 2, session: 7, capacity: 1, edge_capacity: 1 };
        assert(dependency::demand(table, 0, key()).producer > 0);
        assert(dependency::demand(table, 0, key()).producer == 0);
        assert(dependency::demand(table, 1, key()).cell == -1 && dependency::ensure(table, key(2)).cell == -1);
        assert(table.cells.len == 1 && table.edges.len == 1 && table.requests.len == 1);
    }
    print("dependency ordered claims, exact keys, ready edges, once-only terminal delivery and validation/install races passed");
}
