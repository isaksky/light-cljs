//! What a namespace compilation did and how long each phase took: the
//! numbers `--stats` prints (docs/compiler-cache.md, "Implementation
//! sequence", step 1). A compilation collects them only when its caller
//! passes a [`CompileStats`]; without one it reads no clock and records
//! nothing.
//!
//! The report is one `key value` line per counter, then the table of the
//! namespaces with the most macro calls under a `[macro-calls-by-namespace]`
//! line, one `NAMESPACE COUNT` line each. Times are milliseconds with one
//! decimal. Later counters, such as the cache's, are added as further
//! `key value` lines ahead of the table.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

/// The version of the report's format, its first line.
pub const REPORT_VERSION: u32 = 1;

/// How many namespaces the report's table lists.
pub const REPORT_NAMESPACES: usize = 20;

/// Wall time per phase of a compilation. The phases do not overlap; work a
/// phase demands of another (such as a deferred unit a macro needs while a
/// namespace's top level is analyzed) counts in the phase that demanded it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhaseTimes {
    /// Locating, reading and parsing sources and their `ns` forms.
    pub resolve_parse: Duration,
    /// Analyzing each namespace's forms with macro expansion, and closing
    /// the program, except draining its deferred units.
    pub analysis: Duration,
    /// Analyzing and preparing the deferred units each namespace leaves
    /// when its top level is done (the drain before the namespace returns).
    pub deferred_units: Duration,
    /// `prepare_application`.
    pub prepare_application: Duration,
    /// Emitting JavaScript and source maps.
    pub emission: Duration,
    /// Writing the output to stdout or files.
    pub write: Duration,
}

impl PhaseTimes {
    /// The sum of the phases.
    pub fn total(&self) -> Duration {
        self.resolve_parse
            + self.analysis
            + self.deferred_units
            + self.prepare_application
            + self.emission
            + self.write
    }
}

/// One namespace of a compilation, in the order of analysis.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NamespaceStats {
    pub name: String,
    /// Macro calls executed while the namespace was analyzed, its deferred
    /// units included: native, bootstrap and source macros alike, the
    /// expansions `--macro-expansions` limits.
    pub macro_calls: usize,
}

/// The deferred compilation units of a compilation
/// (`src/compilation_session.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeferredUnitCounts {
    /// Units registered: compile-time definitions, each analyzed only on
    /// demand or when its namespace drains.
    pub registered: usize,
    /// Units analyzed.
    pub analyzed: usize,
    /// Analyzed units whose compile-time JavaScript was prepared and whose
    /// IR moved into the program.
    pub prepared: usize,
    /// Units a macro or another unit executed at compile time.
    pub executed: usize,
}

/// The numbers of one compilation; see the module documentation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompileStats {
    pub phases: PhaseTimes,
    /// From the start of the compilation to the end of its last recorded
    /// phase.
    pub wall: Duration,
    /// The namespaces compiled, by source index.
    pub namespaces: Vec<NamespaceStats>,
    /// Macro calls executed in the whole compilation.
    pub macro_calls: usize,
    /// The most form nodes macro expansions and compile-time data readers
    /// had generated, the count `--macro-nodes` limits.
    pub peak_macro_nodes: usize,
    pub deferred_units: DeferredUnitCounts,
    /// `--watch`: loading the program into the watch's development session
    /// after the startup build.
    pub session_load: Option<Duration>,
    /// When namespace artifacts were captured
    /// ([`crate::compiler::analyze_namespace_entry_with_artifacts`]).
    pub capture: Option<CaptureCounts>,
}

/// The namespace artifacts a compilation captured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureCounts {
    /// Namespaces captured as artifacts.
    pub captured: usize,
    /// Namespaces whose artifact capture refused, with a reason.
    pub skipped: usize,
    /// The artifacts' estimated size
    /// ([`crate::namespace_artifact::NamespaceArtifact::approximate_bytes`]).
    pub bytes: usize,
    /// Time spent capturing, outside `analysis`.
    pub time: Duration,
}

impl CompileStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// The macro calls namespace `name` executed, if it was compiled.
    pub fn macro_calls_in(&self, name: &str) -> Option<usize> {
        self.namespaces
            .iter()
            .find(|namespace| namespace.name == name)
            .map(|namespace| namespace.macro_calls)
    }

    /// The `--stats` report.
    pub fn report(&self) -> String {
        let mut out = String::new();
        let mut line = |key: &str, value: &dyn std::fmt::Display| {
            let _ = writeln!(out, "{key} {value}");
        };
        let ms = |duration: Duration| format!("{:.1}", duration.as_secs_f64() * 1000.0);
        line("light-cljs-stats", &REPORT_VERSION);
        line("time.wall-ms", &ms(self.wall));
        line("time.resolve-parse-ms", &ms(self.phases.resolve_parse));
        line("time.analysis-ms", &ms(self.phases.analysis));
        line("time.deferred-units-ms", &ms(self.phases.deferred_units));
        line(
            "time.prepare-application-ms",
            &ms(self.phases.prepare_application),
        );
        line("time.emission-ms", &ms(self.phases.emission));
        line("time.write-ms", &ms(self.phases.write));
        if let Some(duration) = self.session_load {
            line("time.session-load-ms", &ms(duration));
        }
        line("namespaces", &self.namespaces.len());
        line("macro-calls", &self.macro_calls);
        line("macro-nodes.peak", &self.peak_macro_nodes);
        line("deferred-units.registered", &self.deferred_units.registered);
        line("deferred-units.analyzed", &self.deferred_units.analyzed);
        line("deferred-units.prepared", &self.deferred_units.prepared);
        line("deferred-units.executed", &self.deferred_units.executed);
        if let Some(capture) = &self.capture {
            line("cache.capture.namespaces", &capture.captured);
            line("cache.capture.skipped", &capture.skipped);
            line("cache.capture.bytes", &capture.bytes);
            line("cache.capture-ms", &ms(capture.time));
        }
        out.push_str("[macro-calls-by-namespace]\n");
        let mut order = (0..self.namespaces.len()).collect::<Vec<_>>();
        // Most calls first; ties in source order.
        order.sort_by_key(|&index| std::cmp::Reverse(self.namespaces[index].macro_calls));
        for index in order.into_iter().take(REPORT_NAMESPACES) {
            let namespace = &self.namespaces[index];
            let _ = writeln!(out, "{} {}", namespace.name, namespace.macro_calls);
        }
        out
    }
}

/// The start of a timed phase when `on`; without stats no clock is read.
pub(crate) fn start(on: bool) -> Option<Instant> {
    on.then(Instant::now)
}

/// Add the time since `started` to `phase`, when it was timed.
pub(crate) fn add(phase: &mut Duration, started: Option<Instant>) {
    if let Some(started) = started {
        *phase += started.elapsed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_lists_counters_then_the_busiest_namespaces() {
        let stats = CompileStats {
            phases: PhaseTimes {
                analysis: Duration::from_micros(1500),
                ..PhaseTimes::default()
            },
            namespaces: ["a", "b", "c"]
                .iter()
                .zip([1, 5, 1])
                .map(|(name, macro_calls)| NamespaceStats {
                    name: (*name).into(),
                    macro_calls,
                })
                .collect(),
            macro_calls: 7,
            ..CompileStats::default()
        };
        let report = stats.report();
        assert!(report.starts_with("light-cljs-stats 1\n"));
        assert!(report.contains("\ntime.analysis-ms 1.5\n"));
        assert!(report.contains("\nmacro-calls 7\n"));
        assert!(!report.contains("session-load"));
        assert!(report.ends_with("[macro-calls-by-namespace]\nb 5\na 1\nc 1\n"));
    }

    #[test]
    fn report_lists_only_the_twenty_namespaces_with_the_most_calls() {
        // ns0..ns24 make 0..24 calls, except that ns3 and ns4 tie with ns5
        // at the cutoff's edge: the table keeps the earlier of those.
        let calls = |index: usize| if (3..=5).contains(&index) { 5 } else { index };
        let stats = CompileStats {
            namespaces: (0..25)
                .map(|index| NamespaceStats {
                    name: format!("ns{index}"),
                    macro_calls: calls(index),
                })
                .collect(),
            macro_calls: (0..25).map(calls).sum(),
            ..CompileStats::default()
        };
        let report = stats.report();
        let (counters, table) = report.split_once("[macro-calls-by-namespace]\n").unwrap();
        assert!(counters.contains("\nnamespaces 25\n"));
        assert!(counters.contains(&format!("\nmacro-calls {}\n", stats.macro_calls)));
        let rows = table.lines().collect::<Vec<_>>();
        let mut expected = (6..25)
            .rev()
            .map(|index| format!("ns{index} {index}"))
            .collect::<Vec<_>>();
        expected.push("ns3 5".into());
        assert_eq!(rows, expected);
    }
}
