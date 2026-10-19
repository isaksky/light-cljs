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
    /// Units found macro-free at registration, which `--jobs N` analyzes
    /// on pool workers (docs/compiler-concurrency.md, "As built
    /// (light-cljs-338)").
    pub macro_free: usize,
    /// The time spent analyzing macro-free units, on whichever thread.
    pub macro_free_time: std::time::Duration,
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
    /// Macro calls executed in the whole compilation. The expansions
    /// installed namespaces charge against `--macro-expansions` are not
    /// executed and not counted.
    pub macro_calls: usize,
    /// The most form nodes macro expansions and compile-time data readers
    /// had generated, the count `--macro-nodes` limits, including the nodes
    /// installed namespaces charged.
    pub peak_macro_nodes: usize,
    pub deferred_units: DeferredUnitCounts,
    /// `--watch`: loading the program into the watch's development session
    /// after the startup build.
    pub session_load: Option<Duration>,
    /// When namespace artifacts were captured
    /// ([`crate::compiler::analyze_namespace_entry_with_artifacts`]).
    pub capture: Option<CaptureCounts>,
    /// When namespaces were installed from artifacts
    /// ([`crate::compiler::analyze_namespace_entry_recorded`]).
    pub install: Option<InstallCounts>,
    /// When captured artifacts were also encoded and decoded
    /// ([`crate::artifact_codec`]), to measure the binary format.
    pub codec: Option<CodecCounts>,
    /// When every namespace's cache key was computed and its inputs
    /// re-validated ([`crate::cache_key`]), to measure them.
    pub keys: Option<KeyCounts>,
    /// What the persistent cache did (`--cache-dir`,
    /// [`crate::compile_cache`]).
    pub cache: Option<CacheCounts>,
    /// The exclusive-stage trace of the analysis ([`crate::compile_trace`]).
    pub trace: Option<crate::compile_trace::Trace>,
}

/// What a build's persistent cache did: each namespace is a hit, a miss
/// with the first reason its candidates could not be used, or a policy
/// bypass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CacheCounts {
    pub hits: usize,
    /// `(namespace, kind, description)`, the kind being the first key
    /// component that differed (`source`, `dependencies`, ...) or
    /// `no-artifact`, `index`, `unreadable`, `undecodable` or `refused`.
    pub misses: Vec<(String, &'static str, String)>,
    /// `(namespace, reason, path)`, the path naming the namespace a
    /// propagated reason came from and those it came through.
    pub bypasses: Vec<(String, &'static str, String)>,
    /// Artifacts published, and their bytes.
    pub published: usize,
    pub bytes_written: u64,
    /// Missed namespaces that were not published, with the reason.
    pub unpublished: Vec<(String, String)>,
    /// Artifact bytes read, matching or not.
    pub bytes_read: u64,
    /// Reading indexes and artifacts, checking manifests and keys.
    pub validate_time: Duration,
    pub decode_time: Duration,
    /// Encoding and writing artifacts and indexes.
    pub write_time: Duration,
}

impl CacheCounts {
    /// The `key value` lines of the report.
    fn lines(&self, line: &mut dyn FnMut(&str, &dyn std::fmt::Display)) {
        let ms = |duration: Duration| format!("{:.1}", duration.as_secs_f64() * 1000.0);
        line("cache.hits", &self.hits);
        line("cache.misses", &self.misses.len());
        let mut kinds: Vec<&str> = self.misses.iter().map(|(_, kind, _)| *kind).collect();
        kinds.sort_unstable();
        for group in kinds.chunk_by(|a, b| a == b) {
            line(&format!("cache.miss.{}", group[0]), &group.len());
        }
        line("cache.bypasses", &self.bypasses.len());
        let mut reasons: Vec<&str> = self.bypasses.iter().map(|(_, reason, _)| *reason).collect();
        reasons.sort_unstable();
        for group in reasons.chunk_by(|a, b| a == b) {
            line(&format!("cache.bypass.{}", group[0]), &group.len());
        }
        line("cache.published", &self.published);
        line("cache.unpublished", &self.unpublished.len());
        line("cache.bytes-read", &self.bytes_read);
        line("cache.bytes-written", &self.bytes_written);
        line("cache.validate-ms", &ms(self.validate_time));
        line("cache.decode-ms", &ms(self.decode_time));
        line("cache.write-ms", &ms(self.write_time));
    }

    /// The `[cache-misses-and-bypasses]` rows: up to `limit` misses, then up
    /// to `limit` bypasses, each in source order, as `NAMESPACE miss KIND:
    /// DESCRIPTION` and `NAMESPACE bypass REASON PATH`.
    fn rows(&self, out: &mut String, limit: usize) {
        for (name, kind, description) in self.misses.iter().take(limit) {
            let _ = writeln!(out, "{name} miss {kind}: {description}");
        }
        for (name, reason, path) in self.bypasses.iter().take(limit) {
            if path.is_empty() {
                let _ = writeln!(out, "{name} bypass {reason}");
            } else {
                let _ = writeln!(out, "{name} bypass {reason} {path}");
            }
        }
    }
}

/// What computing a compilation's namespace keys cost.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyCounts {
    pub namespaces: usize,
    /// The source text hashed, in bytes.
    pub source_bytes: u64,
    pub time: Duration,
    /// Namespaces whose inputs [`crate::cache_key::revalidate`] found
    /// changed after the compilation read them.
    pub changed: usize,
    pub revalidate_time: Duration,
}

/// What encoding and decoding a compilation's captured artifacts cost.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodecCounts {
    /// Artifacts encoded.
    pub artifacts: usize,
    /// Artifacts that failed to encode or to decode again, which a store
    /// would neither write nor read.
    pub failed: usize,
    /// Their encoded size in all.
    pub bytes: u64,
    /// The largest encoded artifact, and its namespace.
    pub largest: (u64, String),
    /// The source text of the encoded namespaces, in bytes.
    pub source_bytes: u64,
    /// Encoded bytes by section, in [`crate::artifact_codec::Section::ALL`]
    /// order; the header and manifest are the rest of [`Self::bytes`].
    pub sections: [u64; 6],
    /// The longest expression chain of any artifact
    /// ([`crate::namespace_artifact::NamespaceArtifact::ir_depth`]).
    pub ir_depth: u32,
    pub encode_time: Duration,
    pub decode_time: Duration,
}

/// The namespace artifacts a compilation installed in place of analysis.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstallCounts {
    /// The namespaces installed, in source order. Their macro calls are not
    /// executed, so [`CompileStats::macro_calls_in`] counts none for them.
    pub installed: Vec<String>,
    /// The namespaces whose artifact was refused, with the reason; each was
    /// analyzed instead.
    pub refused: Vec<(String, String)>,
    /// Time spent installing, outside `analysis`.
    pub time: Duration,
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

    /// The `--stats-file` trace, with the phases after analysis, when the
    /// compilation was traced.
    pub fn trace_file(&self) -> Option<String> {
        let phases = &self.phases;
        self.trace.as_ref().map(|trace| {
            trace.tsv(&[
                ("prepare-application", phases.prepare_application),
                ("emission", phases.emission),
                ("write", phases.write),
                ("wall", self.wall),
            ])
        })
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
        line("deferred-units.macro-free", &self.deferred_units.macro_free);
        line(
            "time.macro-free-units-ms",
            &ms(self.deferred_units.macro_free_time),
        );
        if let Some(capture) = &self.capture {
            line("cache.capture.namespaces", &capture.captured);
            line("cache.capture.skipped", &capture.skipped);
            line("cache.capture.bytes", &capture.bytes);
            line("cache.capture-ms", &ms(capture.time));
        }
        if let Some(install) = &self.install {
            line("cache.install.namespaces", &install.installed.len());
            line("cache.install.refused", &install.refused.len());
            line("cache.install-ms", &ms(install.time));
        }
        if let Some(codec) = &self.codec {
            line("cache.codec.artifacts", &codec.artifacts);
            line("cache.codec.failed", &codec.failed);
            line("cache.codec.bytes", &codec.bytes);
            line("cache.codec.largest", &codec.largest.0);
            line("cache.codec.source-bytes", &codec.source_bytes);
            line("cache.codec.ir-depth", &codec.ir_depth);
            for (section, bytes) in crate::artifact_codec::Section::ALL
                .iter()
                .zip(codec.sections)
            {
                line(&format!("cache.codec.section.{}", section.name()), &bytes);
            }
            line("cache.encode-ms", &ms(codec.encode_time));
            line("cache.decode-ms", &ms(codec.decode_time));
        }
        if let Some(keys) = &self.keys {
            line("cache.keys.namespaces", &keys.namespaces);
            line("cache.keys.source-bytes", &keys.source_bytes);
            line("cache.keys.changed", &keys.changed);
            line("cache.keys-ms", &ms(keys.time));
            line("cache.revalidate-ms", &ms(keys.revalidate_time));
        }
        if let Some(cache) = &self.cache {
            cache.lines(&mut line);
        }
        if let Some(trace) = &self.trace {
            trace.lines(&mut line);
        }
        out.push_str("[macro-calls-by-namespace]\n");
        let mut order = (0..self.namespaces.len()).collect::<Vec<_>>();
        // Most calls first; ties in source order.
        order.sort_by_key(|&index| std::cmp::Reverse(self.namespaces[index].macro_calls));
        for index in order.into_iter().take(REPORT_NAMESPACES) {
            let namespace = &self.namespaces[index];
            let _ = writeln!(out, "{} {}", namespace.name, namespace.macro_calls);
        }
        if let Some(cache) = &self.cache {
            out.push_str("[cache-misses-and-bypasses]\n");
            cache.rows(&mut out, REPORT_NAMESPACES);
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
        assert!(!report.contains("cache.codec"));
    }

    #[test]
    fn report_lists_codec_counters() {
        let stats = CompileStats {
            codec: Some(CodecCounts {
                artifacts: 3,
                failed: 1,
                bytes: 2100,
                largest: (1000, "app.big".into()),
                source_bytes: 700,
                sections: [600, 50, 400, 30, 20, 10],
                ir_depth: 42,
                encode_time: Duration::from_micros(2500),
                decode_time: Duration::from_millis(2),
            }),
            ..CompileStats::default()
        };
        let report = stats.report();
        let expected = "cache.codec.artifacts 3\n\
                        cache.codec.failed 1\n\
                        cache.codec.bytes 2100\n\
                        cache.codec.largest 1000\n\
                        cache.codec.source-bytes 700\n\
                        cache.codec.ir-depth 42\n\
                        cache.codec.section.strings 600\n\
                        cache.codec.section.declarations 50\n\
                        cache.codec.section.ir 400\n\
                        cache.codec.section.units 30\n\
                        cache.codec.section.diagnostics 20\n\
                        cache.codec.section.names 10\n\
                        cache.encode-ms 2.5\n\
                        cache.decode-ms 2.0\n";
        assert!(report.contains(expected), "{report}");
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
