//! The per-user V8 cache that `vendor/v8/build.rs` reads and fills. A real
//! source build takes hours, so a fixture builder stands in for it and writes
//! its pair where upstream's GN build leaves it in the Cargo output layout.
#[allow(dead_code)]
#[path = "../vendor/v8/cache.rs"]
mod cache;
#[path = "support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

use cache::{BINDING, Engine};
use std::{cell::Cell, fs, path::Path};
use temp_dir::TempDir;

/// A build script's `OUT_DIR`: `target/<profile>/build/v8-<hash>/out`.
fn out_dir(target: &Path, profile: &str, hash: &str) -> std::path::PathBuf {
    let out = target
        .join(profile)
        .join("build")
        .join(format!("v8-{hash}"))
        .join("out");
    fs::create_dir_all(&out).unwrap();
    out
}

#[test]
fn source_build_publishes_reusable_entry() {
    let root = TempDir::new("v8-cache");
    let cache_dir = root.join("cache");
    let entry = cache_dir.join("v8-test-entry");
    fs::create_dir_all(&cache_dir).unwrap();
    let material = "light-cljs patched V8 cache test key\n";
    let archive = b"fixture archive bytes".as_slice();
    let binding = b"// fixture binding\n".as_slice();

    // A miss runs the builder once and publishes what it left in gn_out.
    let first_target = root.join("first-target");
    let first_out = out_dir(&first_target, "debug", "1111");
    let builds = Cell::new(0);
    let engine = cache::obtain(
        &cache_dir,
        &entry,
        material,
        false,
        None,
        &first_out,
        || {
            builds.set(builds.get() + 1);
            let gn_out = first_target.join("debug").join("gn_out");
            fs::create_dir_all(gn_out.join("obj")).unwrap();
            fs::write(gn_out.join("obj").join(cache::static_lib()), archive).unwrap();
            fs::write(gn_out.join(BINDING), binding).unwrap();
        },
    );
    assert_eq!(engine, Engine::Built);
    assert_eq!(builds.get(), 1);
    assert!(
        cache::is_complete(&entry, material),
        "the source build was not published"
    );
    assert_eq!(fs::read(entry.join(cache::static_lib())).unwrap(), archive);
    assert_eq!(fs::read(entry.join(BINDING)).unwrap(), binding);
    assert_eq!(fs::read_to_string(entry.join("key.txt")).unwrap(), material);
    let leftovers: Vec<_> = fs::read_dir(&cache_dir)
        .unwrap()
        .map(|item| item.unwrap().file_name())
        .filter(|name| name != "v8-test-entry")
        .collect();
    assert!(leftovers.is_empty(), "publish left {leftovers:?}");

    // A fresh target directory, even one that requires a hit, reuses the
    // entry without building and gets the binding in its own OUT_DIR.
    let second_out = out_dir(&root.join("second-target"), "debug", "2222");
    let engine = cache::obtain(
        &cache_dir,
        &entry,
        material,
        true,
        None,
        &second_out,
        || panic!("a published entry was built again"),
    );
    assert_eq!(engine, Engine::Cached);
    let copied = cache::take_binding(&entry, &second_out);
    assert_eq!(copied, second_out.join(BINDING));
    assert_eq!(fs::read(&copied).unwrap(), binding);
}

#[test]
#[should_panic(expected = "LIGHT_CLJS_V8_CACHE=require and no cached patched V8 matches")]
fn require_refuses_to_build_on_a_miss() {
    let root = TempDir::new("v8-cache");
    let out = out_dir(&root.join("target"), "debug", "3333");
    cache::obtain(
        &root.join("cache"),
        &root.join("cache").join("v8-missing"),
        "key\n",
        true,
        None,
        &out,
        || panic!("require mode started a source build"),
    );
}
