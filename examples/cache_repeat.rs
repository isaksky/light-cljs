//! Repeated cached builds in one process through the library API
//! (docs/compiler-cache.md, "Validation and performance gates", the row
//! "Repeated builds and teardown"; light-cljs-328). Each build opens the
//! cache in CACHE-DIR, compiles ENTRY with penpot.bb's macro limits and
//! drops what it built. One line per build gives its time, the cache's hits
//! and misses, the program's table sizes, the process's resident set, the
//! resident set once the allocator has returned the pages it freed (macOS
//! `malloc_zone_pressure_relief`), its physical footprint (`vmmap
//! --summary`, the memory it still holds dirty) and what the compiler counts
//! still alive (`compile_stats::live_counts`, light-cljs-380). The last
//! three stay flat when nothing outlives a build, and the counts are zero;
//! the first also counts freed pages the allocator keeps resident for reuse.
//!
//! `cargo run --release --example cache_repeat -- CACHE-DIR BUILDS ENTRY ROOT...`
use light_cljs::compile_cache::CacheOptions;
use light_cljs::compiler::analyze_namespace_entry_cached;
use light_cljs::{CompileStats, MacroHost, analyzer::AnalysisOptions, scheduler::Slots};
use std::{path::PathBuf, process::Command, time::Duration, time::Instant};

/// The physical footprint of this process as `vmmap` prints it, or `-`.
fn footprint() -> String {
    Command::new("vmmap")
        .args(["--summary", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .find_map(|line| Some(line.strip_prefix("Physical footprint:")?.trim().to_owned()))
        })
        .unwrap_or_else(|| "-".to_owned())
}

/// The resident set of this process in bytes, from `ps`.
fn resident_bytes() -> u64 {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u64>()
        .unwrap_or(0)
        * 1024
}

/// Ask every malloc zone to return the free pages it keeps resident.
fn release_free_pages() {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
        }
        // SAFETY: a null zone means every zone, and a goal of 0 as much as
        // possible; the call only returns free pages to the system.
        unsafe {
            malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let usage = "CACHE-DIR BUILDS ENTRY ROOT...";
    let dir = PathBuf::from(args.next().ok_or(usage)?);
    let builds: usize = args.next().ok_or(usage)?.parse()?;
    let entry = args.next().ok_or(usage)?;
    let roots: Vec<PathBuf> = args.map(PathBuf::from).collect();
    let cache = CacheOptions {
        dir,
        blockers: Vec::new(),
        retention: Default::default(),
    };
    println!(
        "build ms hits misses exprs globals strings rss-bytes relieved-rss-bytes footprint live"
    );
    for build in 1..=builds {
        let mut host = MacroHost::new();
        host.opaque_packages = true;
        host.limits.expansions = 100_000_000;
        host.limits.generated_nodes = 4_000_000_000;
        host.evaluator_options.timeout = Duration::from_secs(3600);
        host.evaluator_options.memory_limit = 4_000_000_000;
        host.evaluator_options.poll_budget = 1_000_000_000_000;
        let mut stats = CompileStats::new();
        let mut warnings = Vec::new();
        let started = Instant::now();
        let (program, files) = analyze_namespace_entry_cached(
            &entry,
            &roots,
            &mut host,
            AnalysisOptions::default(),
            &mut warnings,
            &cache,
            Some(&mut stats),
            Slots::ONE,
        )?;
        let millis = started.elapsed().as_secs_f64() * 1000.0;
        let counts = stats.cache.unwrap_or_default();
        let tables = (
            program.exprs.len(),
            program.globals.len(),
            program.strings.len(),
        );
        drop((program, files, host, warnings));
        let live = light_cljs::compile_stats::live_counts();
        let resident = resident_bytes();
        release_free_pages();
        println!(
            "{build} {millis:.0} {} {} {} {} {} {resident} {} {} {}",
            counts.hits,
            counts.misses.len(),
            tables.0,
            tables.1,
            tables.2,
            resident_bytes(),
            footprint(),
            live.summary()
        );
    }
    Ok(())
}
