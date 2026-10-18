//! Namespace artifacts in their binary encoding (docs/compiler-cache.md,
//! "Representation and relocation"). Every artifact the cache-contract
//! fixtures capture already makes the round trip there
//! ([`crate::cache_contract::round_trip`]) and is installed decoded; this
//! suite checks that damaged bytes fail cleanly, as typed decode errors.
use crate::cache_contract::{Input, capture};
use crate::temp_dir::TempDir;
use light_cljs::artifact_codec::{
    self, DecodeError, DecodeLimits, ManifestFields, SCHEMA_VERSION, Section, decode,
    read_manifest, seal,
};
use std::fs;
use std::path::PathBuf;

/// Macros, helper units, a protocol and record, a warning, metadata and
/// special numbers, over two namespaces.
const FIXTURE: [(&str, &str); 2] = [
    (
        "lib.cljs",
        "(ns lib)\n\
         (defn helper [x] (list 'inc x))\n\
         (defmacro bump [x] (helper x))\n\
         (defprotocol Shape (area [s]))\n\
         (defrecord Square [side] Shape (area [_] (* side side)))\n\
         (def ^{:doc \"Special values.\" :tags #{:a [1 2.5 -0.0]}} special\n\
           [##NaN ##-Inf ##Inf -0.0 9007199254740993 \"text\" :k/w 'sym])\n",
    ),
    (
        "main.cljs",
        "(ns main (:require [lib :as l :refer [bump]]))\n\
         (defn f [x] (+ (bump x) 1))\n\
         (js/console.log (f 2) (l/area (l/->Square 3)) (count l/special))\n",
    ),
];

/// Every artifact of [`FIXTURE`], encoded.
fn encoded() -> Vec<(String, Vec<u8>)> {
    let sources = TempDir::new("cljs-artifact-codec");
    for (path, source) in FIXTURE {
        let path = sources.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let roots: Vec<PathBuf> = vec![sources.path().to_path_buf()];
    let captured = capture(Input::Roots(&roots), "main", &[]).expect("the fixture compiles");
    let mut artifacts = captured
        .namespaces
        .iter()
        .filter_map(|name| captured.originals.get(name))
        .map(|artifact| {
            let bytes = artifact_codec::encode(artifact, &ManifestFields::default()).unwrap();
            (artifact.header.namespace.clone(), bytes)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        artifacts
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["lib", "main"]
    );
    // The fixture exercises every section with more than an empty count.
    for (name, bytes) in &artifacts {
        let manifest = read_manifest(bytes, &DecodeLimits::default()).unwrap();
        for section in &manifest.sections {
            assert!(
                section.length > 1,
                "{name} has an empty {:?}",
                section.section
            );
        }
    }
    artifacts.shrink_to_fit();
    artifacts
}

fn decoded(bytes: &[u8]) -> Result<(), DecodeError> {
    decode(bytes, &DecodeLimits::default(), &|| false).map(|_| ())
}

#[test]
fn truncation_at_every_section_boundary_fails_cleanly() {
    for (name, bytes) in encoded() {
        let manifest = read_manifest(&bytes, &DecodeLimits::default()).unwrap();
        let mut cuts = vec![0, 1, 8, 12, 16, 20, 51, 52];
        for section in &manifest.sections {
            let start = section.offset as usize;
            let end = start + section.length as usize;
            cuts.extend([start - 1, start, start + 1, end - 1, end]);
        }
        cuts.retain(|&cut| cut < bytes.len());
        cuts.sort();
        cuts.dedup();
        for cut in cuts {
            let error = decoded(&bytes[..cut]).unwrap_err();
            assert!(
                matches!(error, DecodeError::Truncated(_)),
                "{name} cut at {cut}: {error:?}"
            );
        }
        decoded(&bytes).unwrap();
    }
}

#[test]
fn a_bit_flip_in_any_section_fails_cleanly() {
    for (name, bytes) in encoded() {
        let manifest = read_manifest(&bytes, &DecodeLimits::default()).unwrap();
        let flipped = |at: usize, bit: u8| {
            let mut copy = bytes.clone();
            copy[at] ^= 1 << bit;
            decoded(&copy).unwrap_err()
        };
        for section in &manifest.sections {
            let start = section.offset as usize;
            let length = section.length as usize;
            for at in [start, start + length / 2, start + length - 1] {
                for bit in [0, 7] {
                    assert_eq!(
                        flipped(at, bit),
                        DecodeError::Integrity(section.section.name()),
                        "{name}: bit {bit} of byte {at}"
                    );
                }
            }
        }
        assert_eq!(flipped(0, 3), DecodeError::BadMagic);
        assert_eq!(flipped(12, 0), DecodeError::ByteOrder);
        // The manifest's length, its hash and its body.
        let first = manifest.sections[0].offset as usize;
        for at in [16, 20, 51, 52, (52 + first) / 2, first - 1] {
            let error = flipped(at, 0);
            assert!(
                matches!(
                    error,
                    DecodeError::Integrity("manifest") | DecodeError::Truncated(_)
                ),
                "{name}: byte {at}: {error:?}"
            );
        }
    }
}

#[test]
fn a_wrong_version_is_refused_before_anything_else() {
    for (_, bytes) in encoded() {
        for version in [0, SCHEMA_VERSION + 1, u32::MAX] {
            let mut copy = bytes.clone();
            copy[8..12].copy_from_slice(&version.to_le_bytes());
            // Even sealed again, which a different compiler would have done.
            seal(&mut copy).unwrap();
            assert_eq!(decoded(&copy).unwrap_err(), DecodeError::Version(version));
            assert_eq!(
                read_manifest(&copy, &DecodeLimits::default()).unwrap_err(),
                DecodeError::Version(version)
            );
        }
    }
}

#[test]
fn reader_data_deeper_than_the_limit_is_refused() {
    let limits = DecodeLimits {
        max_depth: 1,
        ..DecodeLimits::default()
    };
    let (_, lib) = &encoded()[0];
    assert_eq!(
        decode(lib, &limits, &|| false).unwrap_err(),
        DecodeError::TooDeep(1)
    );
}

/// A small deterministic generator (xorshift64*).
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// Random damage, with a fixed seed. Damage that is not sealed again always
/// fails, by the integrity hashes or the bounds; damage sealed again, as if
/// the hashes had been recomputed, decodes or fails without panicking, and
/// what decodes is a valid artifact that encodes again.
#[test]
fn randomized_corruption_fails_cleanly() {
    let artifacts = encoded();
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    let mut sealed_decodes = 0;
    let mut sealed_failures = 0;
    for round in 0..4000 {
        let (name, original) = &artifacts[round % artifacts.len()];
        let mut bytes = original.clone();
        // Bias damage into the sections, past the fixed header.
        let at = |random: &mut Random, len: usize| {
            if random.below(4) == 0 {
                random.below(len)
            } else {
                len - 1 - random.below((len / 2).max(1))
            }
        };
        for _ in 0..1 + random.below(3) {
            match random.below(5) {
                0 => {
                    let at = at(&mut random, bytes.len());
                    bytes[at] ^= 1 << random.below(8);
                }
                1 => {
                    let at = at(&mut random, bytes.len());
                    bytes[at] = random.next() as u8;
                }
                2 => {
                    let cut = random.below(bytes.len());
                    bytes.truncate(cut.max(1));
                }
                3 => {
                    let at = at(&mut random, bytes.len());
                    bytes.insert(at, random.next() as u8);
                }
                _ => {
                    let at = at(&mut random, bytes.len());
                    let end = (at + 1 + random.below(8)).min(bytes.len());
                    bytes.drain(at..end);
                }
            }
        }
        if bytes == *original {
            continue;
        }
        let sealed = round % 2 == 1 && seal(&mut bytes).is_ok();
        match decode(&bytes, &DecodeLimits::default(), &|| false) {
            Ok((_, artifact)) => {
                assert!(sealed, "{name} round {round}: unsealed damage decoded");
                artifact_codec::encode(&artifact, &ManifestFields::default()).unwrap();
                sealed_decodes += 1;
            }
            Err(error) => {
                if sealed {
                    sealed_failures += 1;
                }
                assert!(!error.to_string().is_empty());
            }
        }
    }
    // Sealed damage reaches the structural checks, which refuse much of it;
    // damage inside text or a number still decodes to a valid artifact.
    assert!(
        sealed_failures > 100 && sealed_decodes > 0,
        "{sealed_failures} sealed failures, {sealed_decodes} decodes"
    );
}

#[test]
fn every_section_is_listed_in_order_with_its_hash() {
    for (_, bytes) in encoded() {
        let manifest = read_manifest(&bytes, &DecodeLimits::default()).unwrap();
        let sections = manifest
            .sections
            .iter()
            .map(|bounds| bounds.section)
            .collect::<Vec<_>>();
        assert_eq!(sections, Section::ALL);
        for bounds in &manifest.sections {
            let start = bounds.offset as usize;
            assert_eq!(
                light_cljs::sha256::digest(&bytes[start..start + bounds.length as usize]),
                bounds.sha256
            );
        }
        let last = manifest.sections.last().unwrap();
        assert_eq!(u64::from(last.offset + last.length), manifest.length);
    }
}
