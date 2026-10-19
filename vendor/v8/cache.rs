//! The per-user cache of patched V8 builds that `build.rs` reads and fills
//! (README.md). Standard library only: light-cljs's `tests/v8_cache.rs`
//! includes this file to test the miss, build, publish and reuse sequence,
//! because this crate's own `build` test target cannot run in that workspace.
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Bump when this wrapper changes what a source build produces.
const CACHE_SCHEMA: u32 = 1;
/// The source recipe, the engine patch and upstream's GN/bindgen logic.
const KEY_FILES: [&str; 3] = ["prepare.bb", "engine-budget.patch", "upstream_build.rs"];
/// Environment that upstream turns into GN or bindgen arguments.
const KEY_ENV: [&str; 8] = [
    "V8_FORCE_DEBUG",
    "GN_ARGS",
    "EXTRA_GN_ARGS",
    "DISABLE_CLANG",
    "CLANG_BASE_PATH",
    "RUSTY_V8_MUSL_SYSROOT",
    "RUSTY_V8_GLIBC_PREFIX",
    "RUSTY_V8_BINDGEN_RESOURCE_DIR",
];
pub const BINDING: &str = "src_binding.rs";

/// Where a build's engine came from once it holds its entry's lock.
#[derive(Debug, PartialEq, Eq)]
pub enum Engine {
    /// The complete entry, found or just seeded: hand it to upstream.
    Cached,
    /// Built from source into this target directory, and published if possible.
    Built,
}

/// Uses a complete `entry`, else publishes `seed`'s pair as it, else (unless
/// `require`) runs `build` and publishes the pair a source build leaves in
/// `target/<profile>/gn_out` above `out_dir`. A failed publish only warns:
/// the engine was built, and the next build of this key builds it again.
pub fn obtain(
    cache: &Path,
    entry: &Path,
    material: &str,
    require: bool,
    seed: Option<&Path>,
    out_dir: &Path,
    build: impl FnOnce(),
) -> Engine {
    if is_complete(entry, material) {
        return Engine::Cached;
    }
    if let Some(seed) = seed {
        publish(
            cache,
            entry,
            material,
            &seed.join(static_lib()),
            &seed.join(BINDING),
        )
        .unwrap_or_else(|e| panic!("seed the V8 cache from {}: {e}", seed.display()));
        return Engine::Cached;
    }
    assert!(
        !require,
        "LIGHT_CLJS_V8_CACHE=require and no cached patched V8 matches this build: {}",
        entry.display()
    );
    println!(
        "cargo:warning=building the patched V8 from source for {}",
        entry.display()
    );
    build();
    let gn_out = target_profile_dir(out_dir).join("gn_out");
    let built = (gn_out.join("obj").join(static_lib()), gn_out.join(BINDING));
    if let Err(e) = publish(cache, entry, material, &built.0, &built.1) {
        println!(
            "cargo:warning=could not publish the built V8 to {}: {e}",
            entry.display()
        );
    }
    Engine::Built
}

/// Copies the cached binding into `out_dir`, because it is compiled into the
/// crate, and returns the copy's path; upstream copies the archive itself.
pub fn take_binding(entry: &Path, out_dir: &Path) -> PathBuf {
    let binding = out_dir.join(BINDING);
    fs::copy(entry.join(BINDING), &binding).expect("copy the cached V8 binding");
    binding
}

/// Everything that decides the archive and binding, in full: an entry is used
/// only when its recorded material equals this byte for byte.
pub fn key_material(manifest: &Path) -> String {
    let var = |name: &str| env::var(name).unwrap_or_default();
    let mut text = format!("light-cljs patched V8 cache schema {CACHE_SCHEMA}\n");
    // Archives from the two Cargo profiles differ only in the build directory
    // names their source paths embed; keying the profile keeps each faithful.
    for name in ["CARGO_PKG_VERSION", "TARGET", "HOST", "PROFILE"] {
        text.push_str(&format!("{name}={}\n", var(name)));
    }
    let mut features: Vec<String> = env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| name.starts_with("CARGO_FEATURE_"))
        .collect();
    features.sort();
    text.push_str(&format!("features={}\n", features.join(",")));
    text.push_str(&format!("asan={}\n", is_asan()));
    // Presence matters on its own (upstream tests DISABLE_CLANG by presence).
    for name in key_env() {
        match env::var_os(&name) {
            Some(value) => text.push_str(&format!("{name}={}\n", value.to_string_lossy())),
            None => text.push_str(&format!("{name} unset\n")),
        }
    }
    for file in KEY_FILES {
        let content =
            fs::read_to_string(manifest.join(file)).unwrap_or_else(|e| panic!("read {file}: {e}"));
        text.push_str(&format!("file {file} {}\n{content}\n", content.len()));
    }
    text
}

/// `KEY_ENV` plus the extra clang arguments bindgen reads for this target.
pub fn key_env() -> Vec<String> {
    let target = env::var("TARGET").unwrap();
    let mut names: Vec<String> = KEY_ENV.iter().map(|name| name.to_string()).collect();
    names.push("BINDGEN_EXTRA_CLANG_ARGS".into());
    names.push(format!("BINDGEN_EXTRA_CLANG_ARGS_{target}"));
    names.push(format!(
        "BINDGEN_EXTRA_CLANG_ARGS_{}",
        target.replace("-", "_")
    ));
    names
}

pub fn is_asan() -> bool {
    let rustflags = env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    rustflags.contains("-Z sanitizer=address") || rustflags.contains("-Zsanitizer=address")
}

pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The archive's name for the target. Cargo always gives a build script the
/// target OS; outside one (the tests) the host's naming stands in.
pub fn static_lib() -> &'static str {
    let windows = env::var("CARGO_CFG_TARGET_OS").map_or(cfg!(windows), |os| os == "windows");
    if windows {
        "rusty_v8.lib"
    } else {
        "librusty_v8.a"
    }
}

/// `target/<profile>` above `out_dir` (`target/<profile>/build/v8-<hash>/out`),
/// where upstream keeps `gn_out`.
fn target_profile_dir(out_dir: &Path) -> &Path {
    out_dir.ancestors().nth(3).unwrap()
}

fn sizes(dir: &Path) -> io::Result<String> {
    Ok(format!(
        "{} {}\n{BINDING} {}\n",
        static_lib(),
        fs::metadata(dir.join(static_lib()))?.len(),
        fs::metadata(dir.join(BINDING))?.len()
    ))
}

/// An entry appears only by renaming a fully written directory, so a complete
/// one has this key's material and the file sizes it was published with.
pub fn is_complete(entry: &Path, material: &str) -> bool {
    fs::read_to_string(entry.join("key.txt")).is_ok_and(|key| key == material)
        && fs::read_to_string(entry.join("sizes.txt"))
            .is_ok_and(|recorded| sizes(entry).is_ok_and(|actual| actual == recorded))
}

/// Write the pair into a private temporary directory, flush it, then rename
/// it into place. Readers never see a partial entry; an interrupted publish
/// leaves only a `.tmp-` directory that no lookup reads.
fn publish(
    cache: &Path,
    entry: &Path,
    material: &str,
    archive: &Path,
    binding: &Path,
) -> io::Result<()> {
    remove_stale_temporaries(cache);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = cache.join(format!(".tmp-{}-{nanos}", std::process::id()));
    fs::create_dir(&temporary)?;
    let result = (|| {
        fs::copy(archive, temporary.join(static_lib()))?;
        fs::copy(binding, temporary.join(BINDING))?;
        fs::write(temporary.join("key.txt"), material)?;
        fs::write(temporary.join("sizes.txt"), sizes(&temporary)?)?;
        for file in [static_lib(), BINDING, "key.txt", "sizes.txt"] {
            // Windows flushes only through a handle with write access.
            fs::OpenOptions::new()
                .write(true)
                .open(temporary.join(file))?
                .sync_all()?;
        }
        sync_dir(&temporary)?;
        if entry.exists() {
            // Unusable (damaged, or another key's): move it aside first.
            let stale = cache.join(format!(".tmp-stale-{}-{nanos}", std::process::id()));
            fs::rename(entry, &stale)?;
            let _ = fs::remove_dir_all(&stale);
        }
        match fs::rename(&temporary, entry) {
            Err(_) if is_complete(entry, material) => {}
            result => result?,
        }
        sync_dir(cache)
    })();
    if temporary.exists() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    if cfg!(unix) {
        fs::File::open(dir)?.sync_all()
    } else {
        Ok(())
    }
}

/// Temporaries live for seconds; one a day old was left by a killed publish.
fn remove_stale_temporaries(cache: &Path) {
    let Ok(entries) = fs::read_dir(cache) else {
        return;
    };
    for item in entries.flatten() {
        let old = item
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(24 * 60 * 60));
        if old && item.file_name().to_string_lossy().starts_with(".tmp-") {
            let _ = fs::remove_dir_all(item.path());
        }
    }
}
