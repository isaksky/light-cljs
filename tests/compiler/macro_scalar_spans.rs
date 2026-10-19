//! The span of a scalar a macro returns: that of the scalar in the call's
//! own form when the macro returns one equal to it, else that of the call,
//! never that of an equal scalar some earlier export carried.
use crate::temp_dir::TempDir;
use light_cljs::analyzer::{AnalysisOptions, ExprKind};
use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
use light_cljs::scheduler::Slots;
use light_cljs::{ApplicationOptions, MacroHost, prepare_application};
use std::fs;

struct Sources(TempDir);

impl Sources {
    fn new() -> Self {
        Self(TempDir::new("cljs-scalar-spans"))
    }
    fn write(&self, path: &str, source: &str) -> &Self {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
        self
    }
}

type Spans = Vec<(usize, std::ops::Range<usize>)>;

/// The source index and span of every number expression, and the mapped
/// output, of `entry` analyzed at `slots`.
fn build(
    sources: &Sources,
    entry: &str,
    slots: usize,
) -> (Spans, (String, light_cljs::source_map::SourceMapping)) {
    let mut warnings = Vec::new();
    let (program, _) = analyze_namespace_entry_recorded(
        entry,
        std::slice::from_ref(sources.0.path()),
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut warnings,
        EntryRecorders {
            slots: Slots::new(slots).unwrap(),
            ..Default::default()
        },
    )
    .unwrap();
    let numbers = program
        .exprs
        .iter()
        .filter(|e| matches!(e.kind, ExprKind::Number(_)))
        .map(|e| (e.source_index, e.span.clone()))
        .collect();
    let mapped = prepare_application(&program, ApplicationOptions::default()).emit_js_mapped();
    (numbers, mapped)
}

#[test]
fn several_slots_give_a_macros_scalar_the_one_slot_span() {
    let sources = Sources::new();
    sources.write(
        "app/m.cljs",
        "(ns app.m) (defmacro mac [] 1) (defn twice [x] (* 2 x))",
    );
    let mut n0 = String::from("(ns app.n0 (:require [app.m :refer [mac]]))\n");
    for k in 0..12 {
        n0.push_str(&format!(
            "(defn f{k} [x] (let [[a b] [x {k}]] (+ a b 0)))\n(def v{k} (when (f{k} 1) (mac)))\n"
        ));
    }
    sources.write("app/n0.cljs", &n0).write(
        "app/main.cljs",
        "(ns app.main (:require [app.n0])) (js/console.log (app.n0/f11 1) app.n0/v0)",
    );
    let serial = build(&sources, "app.main", 1);
    for slots in [2, 4, 8] {
        let pooled = build(&sources, "app.main", slots);
        assert_eq!(pooled.0, serial.0, "{slots} slots");
        assert_eq!(pooled.1, serial.1, "{slots} slots");
    }
}

#[test]
fn a_macros_new_scalar_has_its_calls_span_after_an_equal_argument_of_an_earlier_call() {
    let sources = Sources::new();
    sources.write(
        "app/m.cljs",
        "(ns app.m) (defmacro same [x] x) (defmacro seven [] 7)",
    );
    let main =
        "(ns app.main (:require [app.m :refer [same seven]]))\n(def a (same 7))\n(def b (seven))";
    sources.write("app/main.cljs", main);
    let (numbers, _) = build(&sources, "app.main", 1);
    let body = main.find("(def a").unwrap();
    let main_index = numbers.last().unwrap().0;
    let spans: Vec<&str> = numbers
        .iter()
        .filter(|(index, span)| *index == main_index && span.start >= body)
        .map(|(_, span)| &main[span.clone()])
        .collect();
    // The argument keeps its own span. The new 7 has its call's span, not
    // that of the equal argument the call before it imported.
    assert_eq!(spans, ["7", "(seven)"]);
}
