//! The versioned binary encoding of a [`NamespaceArtifact`]
//! (docs/compiler-cache.md, "Representation and relocation").
//!
//! An encoded artifact is a header, a manifest and six sections:
//!
//! ```text
//! offset  size
//!      0     8  magic "LCLJSART"
//!      8     4  schema version, u32 little-endian (SCHEMA_VERSION)
//!     12     4  byte-order mark 0x01020304, written little-endian
//!     16     4  manifest length M
//!     20    32  SHA-256 of bytes 0..20 followed by the manifest
//!     52     M  manifest
//!   52+M        sections, contiguous and in kind order, to the end
//! ```
//!
//! Every multi-byte fixed-width number is little-endian. The manifest starts
//! with its section table at a fixed place (a count byte, then per section
//! its kind byte, offset and length as u32 and its SHA-256), followed by
//! what a lookup needs without decoding IR: the namespace's identity, the
//! front-end key with its component digests, the emission key, the
//! compiler's core-operation table, the namespace's requires and referenced
//! namespaces, its dependency edges, the dependencies its front-end key
//! covers with their keys, its `light.build` inputs and its policy flags.
//! Manifest text is inline UTF-8.
//!
//! The sections are [`Section::ALL`]: the artifact's one interned UTF-16
//! string table, then declarations (with the references and patches), the
//! semantic IR, the deferred units, the diagnostics, and the generated names
//! with the compiler-owned accounting. Every text field outside the manifest
//! is an index into the string table; a Rust `String` must be valid UTF-16
//! there, while reader text keeps its code units as they are, lone
//! surrogates included.
//!
//! Inside a section, counts, indices and unsigned numbers are LEB128
//! varints, signed integers zigzag varints, and doubles their IEEE-754 bits
//! as 8 bytes, so `-0.0`, infinities and every NaN payload come back as they
//! were. Every enum is an explicit tag byte. Artifact references stay the
//! artifact's own dense indices ([`crate::namespace_artifact`]), so shared
//! identities (a `VarMetadata::Global`, two expressions naming one binding)
//! remain shared. `GlobalRef` is `2i` for an own global and `2i + 1` for an
//! external one, `SourceRef` 0 for the own source and `n + 1` for a
//! referenced namespace, and an optional index `n + 1`, with 0 for none.
//!
//! Decoding is bounded ([`DecodeLimits`]) and checks before it allocates:
//! no count exceeds the bytes left in its section or the entry limit, no
//! reader datum nests deeper than the depth limit, every string index is
//! inside the table, and the decoded artifact passes
//! [`NamespaceArtifact::validate`]. Long tables poll for cancellation. Every
//! failure is a [`DecodeError`], which a store treats as a miss.
//!
//! Versioning: any change to what is written, including a field added to an
//! artifact type (the encoder destructures every struct, so a new field does
//! not compile until it is encoded), bumps [`SCHEMA_VERSION`]. A decoder reads
//! exactly its own version and nothing else; there is no migration, since a
//! cache artifact is disposable. The meaning of a `CoreFnId` tag is its row
//! in `src/core_operations.tsv`, so the manifest also records a digest of
//! that table and a different one is a mismatch.

use crate::analyzer::CoreFnId;
use crate::namespace_artifact::*;
use crate::sha256::{Sha256, digest};
use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

pub const MAGIC: [u8; 8] = *b"LCLJSART";
/// The encoding's version. Bump it with any change to the bytes written.
pub const SCHEMA_VERSION: u32 = 5;
const BYTE_ORDER: u32 = 0x0102_0304;
const PREFIX: usize = 20;
const MANIFEST_START: usize = PREFIX + 32;
const SECTION_ENTRY: usize = 1 + 4 + 4 + 32;

/// A section of an encoded artifact, in file order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Section {
    Strings,
    Declarations,
    Ir,
    Units,
    Diagnostics,
    Names,
}

impl Section {
    pub const ALL: [Self; 6] = [
        Self::Strings,
        Self::Declarations,
        Self::Ir,
        Self::Units,
        Self::Diagnostics,
        Self::Names,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Strings => "strings",
            Self::Declarations => "declarations",
            Self::Ir => "ir",
            Self::Units => "units",
            Self::Diagnostics => "diagnostics",
            Self::Names => "names",
        }
    }

    fn tag(self) -> u8 {
        self as u8 + 1
    }
}

/// Where a section lies in the encoded bytes, and its integrity hash.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SectionBounds {
    pub section: Section,
    pub offset: u32,
    pub length: u32,
    pub sha256: [u8; 32],
}

/// A dependency of the namespace by name, with the
/// [`crate::dependency_manifest::EdgeKinds`] bits of its edge.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Dependency {
    pub namespace: String,
    pub kinds: u8,
}

/// What the manifest declares beside the artifact's own identity: the keys
/// ([`crate::cache_key`]), the namespace's dependency edges, its external
/// inputs and its policy flags.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ManifestFields {
    /// With its components, so that a lookup names the first that differs.
    pub front_end_key: crate::cache_key::FrontEndKey,
    pub emission_key: [u8; 32],
    pub dependencies: Vec<Dependency>,
    /// The namespace's `light.build` inputs
    /// ([`crate::dependency_manifest::NamespaceRecord::external_inputs`]),
    /// which a lookup checks without decoding a section.
    pub inputs: Vec<crate::dependency_manifest::ExternalInput>,
    /// The namespace depends on every earlier one
    /// ([`crate::dependency_manifest::EdgeKinds::COARSE`]).
    pub coarse: bool,
    pub no_cache: bool,
    pub always: bool,
}

impl ManifestFields {
    /// The dependency edges, external inputs and policy flags of namespace
    /// `id` of a compilation's dependency manifest, with the given keys.
    pub fn of(
        manifest: &crate::dependency_manifest::DependencyManifest,
        id: crate::dependency_manifest::NamespaceId,
        metadata: crate::cache_policy::NamespaceMetadata,
        front_end_key: crate::cache_key::FrontEndKey,
        emission_key: [u8; 32],
    ) -> Self {
        Self {
            front_end_key,
            emission_key,
            dependencies: manifest
                .dependencies(id)
                .map(|(target, kinds)| Dependency {
                    namespace: manifest.record(target).name.clone(),
                    kinds: kinds.0,
                })
                .collect(),
            inputs: manifest.external_inputs(id).cloned().collect(),
            coarse: manifest.record(id).coarse,
            no_cache: metadata.no_cache,
            always: metadata.always,
        }
    }
}

/// An artifact's manifest, readable without decoding its sections.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub namespace: String,
    pub path: String,
    pub source_length: u32,
    pub cljc: bool,
    pub entry: bool,
    /// The digest of the compiler's core-operation table, which gives
    /// `CoreFnId` tags their meaning.
    pub core_table: [u8; 32],
    pub requires: Vec<String>,
    /// The earlier namespaces the artifact references.
    pub references: Vec<String>,
    pub fields: ManifestFields,
    pub sections: Vec<SectionBounds>,
    /// The encoded artifact's size.
    pub length: u64,
}

/// Bounds on what decoding accepts.
#[derive(Clone, Copy, Debug)]
pub struct DecodeLimits {
    /// The encoded size.
    pub max_bytes: u64,
    /// The entries of any one table or list.
    pub max_entries: u64,
    /// The code units of one string.
    pub max_string: u64,
    /// The nesting of reader data.
    pub max_depth: u32,
    /// What the decoded artifact may allocate: every table, list and text
    /// copy is charged before it is allocated, so references to one long
    /// string cannot expand past this.
    pub max_decoded_bytes: u64,
    /// The longest chain of semantic IR expressions
    /// ([`NamespaceArtifact::ir_depth`]).
    pub max_ir_depth: u32,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_entries: 1 << 26,
            max_string: 1 << 28,
            max_depth: 1024,
            max_decoded_bytes: 1 << 32,
            max_ir_depth: 4096,
        }
    }
}

/// Why an encoded artifact could not be used. Each is a cache miss.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodeError {
    BadMagic,
    Version(u32),
    ByteOrder,
    /// The bytes end inside this part.
    Truncated(&'static str),
    /// A size or count beyond its [`DecodeLimits`] bound.
    Limit {
        what: &'static str,
        value: u64,
        limit: u64,
    },
    /// The section table is not the expected sections in order, end to end.
    Layout(&'static str),
    /// A section or the manifest does not match its SHA-256.
    Integrity(&'static str),
    /// An unknown tag.
    Tag {
        what: &'static str,
        tag: u64,
    },
    /// An index outside its table.
    Index {
        what: &'static str,
        index: u64,
        len: u64,
    },
    /// A varint longer than its type.
    Overflow(&'static str),
    /// Text that is not valid UTF-8 (manifest) or UTF-16 (string table).
    Text(&'static str),
    /// Reader data nested deeper than [`DecodeLimits::max_depth`].
    TooDeep(u32),
    /// Bytes left over at the end of a section.
    Trailing(&'static str),
    /// Encoded by a compiler with another core-operation table.
    CoreTable,
    Cancelled,
    /// The decoded artifact fails [`NamespaceArtifact::validate`].
    Invalid(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadMagic => write!(f, "not an artifact"),
            Self::Version(version) => write!(
                f,
                "schema version {version}, this compiler reads {SCHEMA_VERSION}"
            ),
            Self::ByteOrder => write!(f, "byte-order mark mismatch"),
            Self::Truncated(what) => write!(f, "truncated in its {what}"),
            Self::Limit { what, value, limit } => {
                write!(f, "its {what} {value} exceed the limit {limit}")
            }
            Self::Layout(what) => write!(f, "section table: {what}"),
            Self::Integrity(what) => write!(f, "{what} fails its integrity hash"),
            Self::Tag { what, tag } => write!(f, "unknown {what} tag {tag}"),
            Self::Index { what, index, len } => {
                write!(f, "{what} {index} is outside its table of {len}")
            }
            Self::Overflow(what) => write!(f, "{what} overflows"),
            Self::Text(what) => write!(f, "invalid text in its {what}"),
            Self::TooDeep(limit) => write!(f, "reader data nests deeper than {limit}"),
            Self::Trailing(what) => write!(f, "trailing bytes after its {what}"),
            Self::CoreTable => write!(f, "encoded with another core operation table"),
            Self::Cancelled => write!(f, "decoding was cancelled"),
            Self::Invalid(reason) => write!(f, "invalid: {reason}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// The digest of the core operation table this compiler was built with.
pub fn core_table() -> [u8; 32] {
    static TABLE: OnceLock<[u8; 32]> = OnceLock::new();
    *TABLE.get_or_init(|| {
        let mut hash = Sha256::new();
        for id in CoreFnId::ALL {
            hash.update(format!("{id:?}\n").as_bytes());
        }
        hash.finish()
    })
}

// ---------------------------------------------------------------------------
// Encoding

/// Encode `artifact` with `fields` in its manifest. Fails only for reader
/// data nested deeper than the default [`DecodeLimits::max_depth`], which no
/// decoder would accept.
pub fn encode(artifact: &NamespaceArtifact, fields: &ManifestFields) -> Result<Vec<u8>, String> {
    let NamespaceArtifact {
        header,
        declarations,
        ir,
        patches,
        units,
        diagnostics,
        references,
        names,
        accounting,
    } = artifact;
    let Header {
        namespace,
        path,
        source_length,
        cljc,
        source_name,
        entry,
        root,
        policy,
    } = header;
    let mut strings = Interner::default();
    let max_depth = DecodeLimits::default().max_depth;
    let mut sections: Vec<Vec<u8>> = Vec::with_capacity(Section::ALL.len());
    for section in Section::ALL {
        let mut out = Writer {
            bytes: Vec::new(),
            strings: &mut strings,
            max_depth,
            too_deep: false,
        };
        match section {
            // Written once the others have interned their text.
            Section::Strings => {}
            Section::Declarations => {
                out.option_str(source_name.as_deref());
                out.index(root.0);
                out.policy(*policy);
                out.declarations(declarations);
                out.list(patches, |out, patch| {
                    let GlobalPatch {
                        global,
                        flags,
                        metadata,
                    } = patch;
                    out.global(*global);
                    out.flags(*flags);
                    out.option_index(metadata.map(|e| e.0));
                });
                let References {
                    namespaces: _,
                    globals,
                } = references;
                out.list(globals, |out, global| {
                    let ExternalGlobal {
                        namespace,
                        name,
                        declared,
                        private,
                    } = global;
                    out.str(namespace);
                    out.str(name);
                    out.bool(*declared);
                    out.option_bool(*private);
                });
            }
            Section::Ir => out.ir(ir),
            Section::Units => out.deferred_units(units),
            Section::Diagnostics => out.diagnostics(diagnostics),
            Section::Names => {
                out.names(names);
                match accounting {
                    None => out.u8(0),
                    Some(Accounting {
                        expansions,
                        generated_nodes,
                        node_peak,
                        macro_depth,
                        analysis_depth,
                        unit_depth,
                        conversion_peak,
                    }) => {
                        out.u8(1);
                        out.uint(*expansions);
                        out.uint(*generated_nodes);
                        out.uint(*node_peak);
                        out.index(*macro_depth);
                        out.index(*analysis_depth);
                        out.index(*unit_depth);
                        out.uint(*conversion_peak);
                    }
                }
            }
        }
        if out.too_deep {
            return Err(format!("reader data nests deeper than {max_depth}"));
        }
        sections.push(out.bytes);
    }
    sections[0] = strings.encode();

    let mut manifest = Vec::new();
    manifest.push(Section::ALL.len() as u8);
    manifest.resize(1 + SECTION_ENTRY * Section::ALL.len(), 0);
    let put_str = |manifest: &mut Vec<u8>, text: &str| {
        varint(manifest, text.len() as u64);
        manifest.extend_from_slice(text.as_bytes());
    };
    put_str(&mut manifest, namespace);
    put_str(&mut manifest, path);
    varint(&mut manifest, u64::from(*source_length));
    manifest.push(u8::from(*cljc) | u8::from(*entry) << 1);
    manifest.extend_from_slice(&fields.front_end_key.digest);
    for component in &fields.front_end_key.components {
        manifest.extend_from_slice(component);
    }
    manifest.extend_from_slice(&fields.emission_key);
    manifest.extend_from_slice(&core_table());
    varint(&mut manifest, declarations.requires.len() as u64);
    for name in &declarations.requires {
        put_str(&mut manifest, name);
    }
    varint(&mut manifest, references.namespaces.len() as u64);
    for name in &references.namespaces {
        put_str(&mut manifest, name);
    }
    varint(&mut manifest, fields.dependencies.len() as u64);
    for dependency in &fields.dependencies {
        put_str(&mut manifest, &dependency.namespace);
        manifest.push(dependency.kinds);
    }
    varint(
        &mut manifest,
        fields.front_end_key.dependencies.len() as u64,
    );
    for dependency in &fields.front_end_key.dependencies {
        put_str(&mut manifest, &dependency.namespace);
        manifest.push(dependency.kinds);
        manifest.extend_from_slice(&dependency.key);
    }
    external_inputs(&mut manifest, &fields.inputs)?;
    manifest.push(
        u8::from(fields.coarse) | u8::from(fields.no_cache) << 1 | u8::from(fields.always) << 2,
    );

    let body: usize = sections.iter().map(Vec::len).sum();
    let mut bytes = Vec::with_capacity(MANIFEST_START + manifest.len() + body);
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
    bytes.extend_from_slice(&BYTE_ORDER.to_le_bytes());
    let manifest_length =
        u32::try_from(manifest.len()).map_err(|_| "the manifest exceeds 4 GiB".to_string())?;
    bytes.extend_from_slice(&manifest_length.to_le_bytes());
    bytes.extend_from_slice(&[0; 32]);
    let table = bytes.len() + 1;
    bytes.extend_from_slice(&manifest);
    for (index, (section, content)) in Section::ALL.iter().zip(&sections).enumerate() {
        let offset = u32::try_from(bytes.len()).map_err(|_| "the artifact exceeds 4 GiB")?;
        let entry = table + index * SECTION_ENTRY;
        bytes[entry] = section.tag();
        bytes[entry + 1..entry + 5].copy_from_slice(&offset.to_le_bytes());
        bytes[entry + 5..entry + 9].copy_from_slice(&(content.len() as u32).to_le_bytes());
        bytes.extend_from_slice(content);
    }
    u32::try_from(bytes.len()).map_err(|_| "the artifact exceeds 4 GiB".to_string())?;
    seal(&mut bytes).map_err(|error| format!("sealing the artifact: {error}"))?;
    Ok(bytes)
}

/// The manifest's external inputs: a count, then per input a tag (0 a
/// resource, 1 a namespace, 2 a provider, 3 the configuration's fingerprint,
/// 4 a broad view) and its fields. A resource is its path, its candidates
/// (each a path and a presence byte), a found byte (0 absent, 1 found
/// without an identity, 2 found with one, then the identity), for a found
/// one its SHA-256 as 32 bytes and its length, and a stability byte.
/// Candidate paths must be UTF-8. The configuration is its SHA-256 as 32
/// bytes, and a view one byte (0 namespaces, 1 declarations, 2 config).
fn external_inputs(
    manifest: &mut Vec<u8>,
    inputs: &[crate::dependency_manifest::ExternalInput],
) -> Result<(), String> {
    use crate::dependency_manifest::{
        BroadView, ExternalInput, ResolvedResource, ResourceDependency,
    };
    let put_str = |manifest: &mut Vec<u8>, text: &str| {
        varint(manifest, text.len() as u64);
        manifest.extend_from_slice(text.as_bytes());
    };
    varint(manifest, inputs.len() as u64);
    for input in inputs {
        match input {
            ExternalInput::Resource(ResourceDependency {
                path,
                candidates,
                found,
                stable,
            }) => {
                manifest.push(0);
                put_str(manifest, path);
                varint(manifest, candidates.len() as u64);
                for crate::dependency_manifest::Candidate { path, present } in candidates {
                    let text = path.to_str().ok_or_else(|| {
                        format!("resource candidate {} is not UTF-8", path.display())
                    })?;
                    put_str(manifest, text);
                    manifest.push(u8::from(*present));
                }
                match found {
                    None => manifest.push(0),
                    Some(ResolvedResource {
                        identity,
                        sha256,
                        bytes,
                    }) => {
                        match identity {
                            None => manifest.push(1),
                            Some(identity) => {
                                manifest.push(2);
                                put_str(manifest, identity);
                            }
                        }
                        manifest.extend_from_slice(sha256);
                        varint(manifest, *bytes);
                    }
                }
                manifest.push(u8::from(*stable));
            }
            ExternalInput::Namespace(name) => {
                manifest.push(1);
                put_str(manifest, name);
            }
            ExternalInput::Provider { token, fingerprint } => {
                manifest.push(2);
                put_str(manifest, token);
                put_str(manifest, fingerprint);
            }
            ExternalInput::Configuration { sha256 } => {
                manifest.push(3);
                manifest.extend_from_slice(sha256);
            }
            ExternalInput::View(view) => {
                manifest.push(4);
                manifest.push(match view {
                    BroadView::Namespaces => 0,
                    BroadView::Declarations => 1,
                    BroadView::Config => 2,
                });
            }
        }
    }
    Ok(())
}

fn read_external_inputs(
    input: &mut Input<'_>,
    budget: &Budget,
) -> Result<Vec<crate::dependency_manifest::ExternalInput>, DecodeError> {
    use crate::dependency_manifest::{
        BroadView, Candidate, ExternalInput, ResolvedResource, ResourceDependency,
    };
    let flag = |input: &mut Input<'_>, what: &'static str| match input.byte()? {
        0 => Ok(false),
        1 => Ok(true),
        tag => Err(DecodeError::Tag {
            what,
            tag: tag.into(),
        }),
    };
    let count = input.count()?;
    let mut inputs = budget.vec(count)?;
    for _ in 0..count {
        inputs.push(match input.byte()? {
            0 => {
                let path = input.utf8_charged(budget)?;
                let count = input.count()?;
                let mut candidates = budget.vec(count)?;
                for _ in 0..count {
                    candidates.push(Candidate {
                        path: input.utf8_charged(budget)?.into(),
                        present: flag(input, "candidate presence")?,
                    });
                }
                let found = match input.byte()? {
                    0 => None,
                    tag @ (1 | 2) => {
                        let identity = if tag == 2 {
                            Some(input.utf8_charged(budget)?)
                        } else {
                            None
                        };
                        let sha256 = input.hash()?;
                        Some(ResolvedResource {
                            identity,
                            sha256,
                            bytes: input.uint()?,
                        })
                    }
                    tag => {
                        return Err(DecodeError::Tag {
                            what: "resource answer",
                            tag: tag.into(),
                        });
                    }
                };
                ExternalInput::Resource(ResourceDependency {
                    path,
                    candidates,
                    found,
                    stable: flag(input, "resource stability")?,
                })
            }
            1 => ExternalInput::Namespace(input.utf8_charged(budget)?),
            2 => ExternalInput::Provider {
                token: input.utf8_charged(budget)?,
                fingerprint: input.utf8_charged(budget)?,
            },
            3 => ExternalInput::Configuration {
                sha256: input.hash()?,
            },
            4 => ExternalInput::View(match input.byte()? {
                0 => BroadView::Namespaces,
                1 => BroadView::Declarations,
                2 => BroadView::Config,
                tag => {
                    return Err(DecodeError::Tag {
                        what: "broad view",
                        tag: tag.into(),
                    });
                }
            }),
            tag => {
                return Err(DecodeError::Tag {
                    what: "external input",
                    tag: tag.into(),
                });
            }
        });
    }
    Ok(inputs)
}

/// Recompute the section hashes and the manifest hash of encoded bytes
/// from their section table, which must lie within them.
pub fn seal(bytes: &mut [u8]) -> Result<(), DecodeError> {
    let manifest_length = manifest_length(bytes)?;
    let count = *bytes
        .get(MANIFEST_START)
        .ok_or(DecodeError::Truncated("manifest"))? as usize;
    if 1 + count * SECTION_ENTRY > manifest_length {
        return Err(DecodeError::Truncated("section table"));
    }
    for index in 0..count {
        let entry = MANIFEST_START + 1 + index * SECTION_ENTRY;
        let offset = le32(&bytes[entry + 1..]) as usize;
        let length = le32(&bytes[entry + 5..]) as usize;
        let content = bytes
            .get(offset..offset.saturating_add(length))
            .ok_or(DecodeError::Truncated("sections"))?;
        let hash = digest(content);
        bytes[entry + 9..entry + 41].copy_from_slice(&hash);
    }
    let hash = manifest_hash(bytes, manifest_length);
    bytes[PREFIX..MANIFEST_START].copy_from_slice(&hash);
    Ok(())
}

fn manifest_length(bytes: &[u8]) -> Result<usize, DecodeError> {
    if bytes.len() < MANIFEST_START {
        return Err(DecodeError::Truncated("header"));
    }
    let length = le32(&bytes[16..]) as usize;
    if bytes.len() - MANIFEST_START < length {
        return Err(DecodeError::Truncated("manifest"));
    }
    Ok(length)
}

fn manifest_hash(bytes: &[u8], manifest_length: usize) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(&bytes[..PREFIX]);
    hash.update(&bytes[MANIFEST_START..MANIFEST_START + manifest_length]);
    hash.finish()
}

fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// The artifact's string table: each distinct text once, by first use.
#[derive(Default)]
struct Interner {
    ids: HashMap<Box<[u16]>, u32>,
    order: Vec<Box<[u16]>>,
    scratch: Vec<u16>,
}

impl Interner {
    fn units(&mut self, units: &[u16]) -> u32 {
        if let Some(&id) = self.ids.get(units) {
            return id;
        }
        let id = self.order.len() as u32;
        let text: Box<[u16]> = units.into();
        self.ids.insert(text.clone(), id);
        self.order.push(text);
        id
    }

    fn str(&mut self, text: &str) -> u32 {
        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        scratch.extend(text.encode_utf16());
        let id = self.units(&scratch);
        self.scratch = scratch;
        id
    }

    fn encode(&self) -> Vec<u8> {
        let units: usize = self.order.iter().map(|text| text.len()).sum();
        let mut out = Vec::with_capacity(units * 2 + self.order.len() * 2 + 5);
        varint(&mut out, self.order.len() as u64);
        for text in &self.order {
            varint(&mut out, text.len() as u64);
            for unit in text.iter() {
                out.extend_from_slice(&unit.to_le_bytes());
            }
        }
        out
    }
}

struct Writer<'a> {
    bytes: Vec<u8>,
    strings: &'a mut Interner,
    max_depth: u32,
    too_deep: bool,
}

impl Writer<'_> {
    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }
    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }
    fn uint(&mut self, value: u64) {
        varint(&mut self.bytes, value);
    }
    fn index(&mut self, value: u32) {
        self.uint(u64::from(value));
    }
    fn int(&mut self, value: i64) {
        self.uint(((value << 1) ^ (value >> 63)) as u64);
    }
    fn f64(&mut self, value: f64) {
        self.bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    fn option_index(&mut self, value: Option<u32>) {
        self.uint(value.map_or(0, |v| u64::from(v) + 1));
    }
    fn option_bool(&mut self, value: Option<bool>) {
        self.u8(match value {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        });
    }
    fn str(&mut self, text: &str) {
        let id = self.strings.str(text);
        self.index(id);
    }
    fn units(&mut self, units: &[u16]) {
        let id = self.strings.units(units);
        self.index(id);
    }
    fn option_str(&mut self, text: Option<&str>) {
        match text {
            None => self.u8(0),
            Some(text) => {
                self.u8(1);
                self.str(text);
            }
        }
    }
    fn option_units(&mut self, units: Option<&[u16]>) {
        match units {
            None => self.u8(0),
            Some(units) => {
                self.u8(1);
                self.units(units);
            }
        }
    }
    fn list<T>(&mut self, items: &[T], mut each: impl FnMut(&mut Self, &T)) {
        self.uint(items.len() as u64);
        for item in items {
            each(self, item);
        }
    }
    fn strs(&mut self, items: &[String]) {
        self.list(items, |out, text| out.str(text));
    }
    fn pairs(&mut self, items: &[(String, String)]) {
        self.list(items, |out, (a, b)| {
            out.str(a);
            out.str(b);
        });
    }
    fn triples(&mut self, items: &[(String, String, String)]) {
        self.list(items, |out, (a, b, c)| {
            out.str(a);
            out.str(b);
            out.str(c);
        });
    }
    fn bytes_range(&mut self, (start, end): Bytes) {
        self.index(start);
        self.index(end);
    }
    fn source(&mut self, source: SourceRef) {
        self.option_index(match source {
            SourceRef::Own => None,
            SourceRef::Namespace(index) => Some(index),
        });
    }
    fn global(&mut self, global: GlobalRef) {
        self.uint(match global {
            GlobalRef::Own(index) => u64::from(index) << 1,
            GlobalRef::External(index) => (u64::from(index) << 1) | 1,
        });
    }
    fn option_global(&mut self, global: Option<GlobalRef>) {
        match global {
            None => self.u8(0),
            Some(global) => {
                self.u8(1);
                self.global(global);
            }
        }
    }
    fn exprs(&mut self, items: &[ExprRef]) {
        self.list(items, |out, e| out.index(e.0));
    }
    fn locals(&mut self, items: &[LocalRef]) {
        self.list(items, |out, l| out.index(l.0));
    }
    fn flags(&mut self, flags: GlobalFlags) {
        let GlobalFlags {
            type_descriptor,
            declared,
            constant,
            define,
            boolean_tag,
        } = flags;
        self.u8(u8::from(type_descriptor)
            | u8::from(declared) << 1
            | u8::from(constant) << 2
            | u8::from(define) << 3
            | u8::from(boolean_tag) << 4);
    }
    fn site(&mut self, site: &Site) {
        let Site { source, bytes } = site;
        self.str(source);
        self.bytes_range(*bytes);
    }
    fn code(&mut self, code: &RelocatableCode) {
        let RelocatableCode { text, units } = code;
        self.str(text);
        self.list(units, |out, &(range, unit)| {
            out.bytes_range(range);
            out.index(unit.0);
        });
    }
    fn reader_context(&mut self, context: &ReaderContext) {
        let ReaderContext {
            aliases,
            refers,
            excludes,
            macros,
        } = context;
        self.pairs(aliases);
        self.triples(refers);
        self.strs(excludes);
        self.strs(macros);
    }
    fn view(&mut self, view: &RegistrationView) {
        let RegistrationView {
            own_globals,
            added,
            reader_context,
        } = view;
        self.index(*own_globals);
        self.list(added, |out, &g| out.global(g));
        match reader_context {
            None => self.u8(0),
            Some(context) => {
                self.u8(1);
                self.reader_context(context);
            }
        }
    }
    fn protocol_method(&mut self, method: &ProtocolMethod) {
        let ProtocolMethod {
            global,
            key,
            arities,
            variadic,
        } = method;
        self.global(*global);
        self.str(key);
        self.list(arities, |out, &a| out.index(a));
        self.option_index(*variadic);
    }
    fn datum(&mut self, datum: &Datum, depth: u32) {
        if depth >= self.max_depth {
            self.too_deep = true;
            return;
        }
        let Datum { kind, metadata } = datum;
        match kind {
            DatumKind::Nil => self.u8(0),
            DatumKind::Boolean(value) => {
                self.u8(1);
                self.bool(*value);
            }
            DatumKind::Integer(value) => {
                self.u8(2);
                self.int(*value);
            }
            DatumKind::Double(value) => {
                self.u8(3);
                self.f64(*value);
            }
            DatumKind::Ratio {
                numerator,
                denominator,
            } => {
                self.u8(4);
                self.int(*numerator);
                self.int(*denominator);
            }
            DatumKind::Character(unit) => {
                self.u8(5);
                self.uint(u64::from(*unit));
            }
            DatumKind::String(units) => {
                self.u8(6);
                self.units(units);
            }
            DatumKind::Symbol(namespace, name) => {
                self.u8(7);
                self.option_units(namespace.as_deref());
                self.units(name);
            }
            DatumKind::Keyword(namespace, name) => {
                self.u8(8);
                self.option_units(namespace.as_deref());
                self.units(name);
            }
            DatumKind::Collection { kind, children } => {
                self.u8(9);
                self.u8(collection_tag(*kind));
                self.list(children, |out, child| out.datum(child, depth + 1));
            }
            DatumKind::CompilerLiteral { kind, payload } => {
                self.u8(10);
                self.u8(literal_tag(*kind));
                self.datum(payload, depth + 1);
            }
        }
        match metadata {
            None => self.u8(0),
            Some(metadata) => {
                self.u8(1);
                self.datum(metadata, depth + 1);
            }
        }
    }
    fn option_datum(&mut self, datum: Option<&Datum>) {
        match datum {
            None => self.u8(0),
            Some(datum) => {
                self.u8(1);
                self.datum(datum, 0);
            }
        }
    }

    fn declarations(&mut self, declarations: &Declarations) {
        let Declarations {
            globals,
            protocols,
            record_constructors,
            field_bindings,
            type_methods,
            macro_functions,
            macros,
            aliases,
            requires: _,
            refers,
            excludes,
            js_refers,
            reader_context,
            imports,
            renames,
            namespace_imports,
            namespace_symbol,
            namespace_doc,
            lazy_targets,
        } = declarations;
        self.list(globals, |out, global| {
            let Global {
                namespace,
                name,
                flags,
                metadata,
                span,
                source,
                private,
                binding,
            } = global;
            out.str(namespace);
            out.str(name);
            out.flags(*flags);
            out.option_index(metadata.map(|e| e.0));
            out.bytes_range(*span);
            out.source(*source);
            out.option_bool(*private);
            out.u8(match binding {
                None => 0,
                Some(BindingKind::Value) => 1,
                Some(BindingKind::Macro) => 2,
            });
        });
        self.list(protocols, |out, (protocol, methods)| {
            out.global(*protocol);
            out.list(methods, Self::protocol_method);
        });
        self.list(record_constructors, |out, &g| out.global(g));
        self.list(field_bindings, |out, &(local, target, mutable)| {
            out.index(local.0);
            out.index(target.0);
            out.bool(mutable);
        });
        self.list(type_methods, |out, r| out.index(r.0));
        self.exprs(macro_functions);
        self.list(macros, |out, declaration| {
            let MacroDeclaration {
                name,
                generation,
                metadata,
                definition,
                code,
                view,
            } = declaration;
            out.str(name);
            out.index(*generation);
            out.datum(metadata, 0);
            out.site(definition);
            out.code(code);
            out.view(view);
        });
        self.pairs(aliases);
        self.triples(refers);
        self.strs(excludes);
        self.list(js_refers, |out, (local, refer)| {
            out.str(local);
            match refer {
                JsRefer::Global(name) => {
                    out.u8(0);
                    out.str(name);
                }
                JsRefer::Package(package, export) => {
                    out.u8(1);
                    out.str(package);
                    out.option_str(export.as_deref());
                }
            }
        });
        self.reader_context(reader_context);
        self.list(imports, |out, import| {
            let Import {
                namespace,
                alias,
                refers,
            } = import;
            out.str(namespace);
            out.option_str(alias.as_deref());
            out.strs(refers);
        });
        self.triples(renames);
        self.pairs(namespace_imports);
        self.option_datum(namespace_symbol.as_ref());
        self.option_datum(namespace_doc.as_ref());
        self.list(lazy_targets, |out, target| {
            let LazyTarget {
                site,
                namespace,
                global,
            } = target;
            out.index(site.0);
            out.str(namespace);
            out.option_global(*global);
        });
    }

    fn ir(&mut self, ir: &SemanticIr) {
        let SemanticIr {
            exprs,
            locals,
            recur_targets,
            strings,
            named_literals,
            js_globals,
        } = ir;
        self.list(strings, |out, units| out.units(units));
        self.list(named_literals, |out, literal| {
            let NamedLiteral { namespace, name } = literal;
            out.option_index(namespace.map(|s| s.0));
            out.index(name.0);
        });
        self.list(js_globals, |out, path| out.strs(path));
        self.list(locals, |out, binding| {
            let LocalBinding { name, span, source } = binding;
            out.str(name);
            out.bytes_range(*span);
            out.source(*source);
        });
        self.list(recur_targets, |out, target| {
            let RecurTarget {
                params,
                protocol_impl,
                receiver_from_this,
                used,
                argument_slots,
            } = target;
            out.locals(params);
            out.u8(u8::from(*protocol_impl)
                | u8::from(*receiver_from_this) << 1
                | u8::from(*used) << 2);
            out.list(argument_slots, |out, &slot| out.option_index(slot));
        });
        self.list(exprs, |out, expr| {
            let Expr {
                kind,
                span,
                context,
                source,
            } = expr;
            out.u8(match context {
                ExprContext::Value => 0,
                ExprContext::Discard => 1,
                ExprContext::Return => 2,
            });
            out.bytes_range(*span);
            out.source(*source);
            out.kind(kind);
        });
    }

    fn fn_method(&mut self, method: &FnMethod) {
        let FnMethod {
            params,
            rest,
            body,
            target,
            captures,
            asynchronous,
        } = method;
        self.locals(params);
        self.option_index(rest.map(|l| l.0));
        self.index(body.0);
        self.index(target.0);
        self.locals(captures);
        self.bool(*asynchronous);
    }

    fn methods(&mut self, methods: &[(MethodDispatch, FnMethod)]) {
        self.list(methods, |out, (dispatch, method)| {
            match dispatch {
                MethodDispatch::Protocol(name) => {
                    out.u8(0);
                    out.str(name);
                }
                MethodDispatch::Builtin(name) => {
                    out.u8(1);
                    out.str(name);
                }
            }
            out.fn_method(method);
        });
    }

    fn bindings(&mut self, bindings: &[(LocalRef, ExprRef)]) {
        self.list(bindings, |out, (local, value)| {
            out.index(local.0);
            out.index(value.0);
        });
    }

    fn kind(&mut self, kind: &ExprKind) {
        use ExprKind as K;
        let e = |out: &mut Self, e: &ExprRef| out.index(e.0);
        match kind {
            K::Nil => self.u8(0),
            K::Bool(value) => {
                self.u8(1);
                self.bool(*value);
            }
            K::Number(value) => {
                self.u8(2);
                self.f64(*value);
            }
            K::String(s) => {
                self.u8(3);
                self.index(s.0);
            }
            K::ReaderLiteral {
                kind,
                items,
                quoted,
            } => {
                self.u8(4);
                self.u8(literal_tag(*kind));
                self.exprs(items);
                self.bool(*quoted);
            }
            K::DefinitionTest { value, unit } => {
                self.u8(5);
                e(self, value);
                self.index(unit.0);
            }
            K::WithMeta { value, metadata } => {
                self.u8(6);
                e(self, value);
                e(self, metadata);
            }
            K::Var {
                definition,
                value,
                symbol,
                metadata,
            } => {
                self.u8(7);
                self.bool(*definition);
                e(self, value);
                self.index(symbol.0);
                match metadata {
                    None => self.u8(0),
                    Some(VarMetadata::Expr(id)) => {
                        self.u8(1);
                        e(self, id);
                    }
                    Some(VarMetadata::Global(global)) => {
                        self.u8(2);
                        self.global(*global);
                    }
                }
            }
            K::Keyword(n) => {
                self.u8(8);
                self.index(n.0);
            }
            K::Symbol(n) => {
                self.u8(9);
                self.index(n.0);
            }
            K::List(items) => {
                self.u8(10);
                self.exprs(items);
            }
            K::Vector(items) => {
                self.u8(11);
                self.exprs(items);
            }
            K::Map(items) => {
                self.u8(12);
                self.exprs(items);
            }
            K::Set(items) => {
                self.u8(13);
                self.exprs(items);
            }
            K::LiteralMap(items) => {
                self.u8(14);
                self.exprs(items);
            }
            K::LiteralSet(items) => {
                self.u8(15);
                self.exprs(items);
            }
            K::CoreFn(id) => {
                self.u8(16);
                self.uint(id.table_row() as u64);
            }
            K::Local(l) => {
                self.u8(17);
                self.index(l.0);
            }
            K::Global(g) => {
                self.u8(18);
                self.global(*g);
            }
            K::JsGlobal(j) => {
                self.u8(19);
                self.index(j.0);
            }
            K::ThisAs { binding, body } => {
                self.u8(20);
                self.index(binding.0);
                e(self, body);
            }
            K::JsArguments => self.u8(21),
            K::JsTemplate { segments, args } => {
                self.u8(22);
                self.strs(segments);
                self.exprs(args);
            }
            K::JsIndex { receiver, indices } => {
                self.u8(23);
                e(self, receiver);
                self.exprs(indices);
            }
            K::JsDelete { receiver, key } => {
                self.u8(24);
                e(self, receiver);
                e(self, key);
            }
            K::QualifiedMethod { class, method } => {
                self.u8(25);
                e(self, class);
                self.option_str(method.as_deref());
            }
            K::CopyArguments(value) => {
                self.u8(26);
                e(self, value);
            }
            K::DefProtocol {
                global,
                methods,
                via_metadata,
            } => {
                self.u8(27);
                self.global(*global);
                self.list(methods, Self::protocol_method);
                self.bool(*via_metadata);
            }
            K::ProtocolPredicate {
                protocol,
                value,
                key,
                direct,
            } => {
                self.u8(28);
                e(self, protocol);
                e(self, value);
                self.str(key);
                self.bool(*direct);
            }
            K::ExtendType {
                instance,
                return_target,
                target,
                methods,
            } => {
                self.u8(29);
                self.bool(*instance);
                self.bool(*return_target);
                e(self, target);
                self.methods(methods);
            }
            K::Reify {
                metadata,
                captures,
                methods,
            } => {
                self.u8(30);
                self.option_index(metadata.map(|m| m.0));
                self.locals(captures);
                self.methods(methods);
            }
            K::DefType {
                record,
                global,
                methods,
                layout,
            } => {
                self.u8(31);
                self.bool(*record);
                self.global(*global);
                self.methods(methods);
                let TypeLayout {
                    constructor,
                    body,
                    masks,
                    map_constructor,
                    fields,
                } = &**layout;
                self.option_global(*constructor);
                self.option_index(body.map(|b| b.0));
                self.option_index(masks.map(|m| m.0));
                self.option_global(*map_constructor);
                self.strs(fields);
            }
            K::Construct { callee, args } => {
                self.u8(32);
                e(self, callee);
                self.exprs(args);
            }
            K::Assert {
                test,
                message,
                form,
            } => {
                self.u8(33);
                e(self, test);
                self.option_index(message.map(|m| m.0));
                self.index(form.0);
            }
            K::Def {
                global,
                init,
                test,
                once,
            } => {
                self.u8(34);
                self.global(*global);
                self.option_index(init.map(|i| i.0));
                self.option_index(test.map(|t| t.0));
                self.bool(*once);
            }
            K::SetGlobal { target, value } => {
                self.u8(35);
                e(self, target);
                e(self, value);
            }
            K::Binding { bindings, body } => {
                self.u8(36);
                self.list(bindings, |out, (var, value)| {
                    out.index(var.0);
                    out.index(value.0);
                });
                e(self, body);
            }
            K::WithOutStr(value) => {
                self.u8(37);
                e(self, value);
            }
            K::Exists(value) => {
                self.u8(38);
                e(self, value);
            }
            K::Loadable { target, class } => {
                self.u8(39);
                self.index(target.0);
                e(self, class);
            }
            K::LoadableType => self.u8(40),
            K::BindingExists(value) => {
                self.u8(41);
                e(self, value);
            }
            K::BooleanHint(value) => {
                self.u8(42);
                e(self, value);
            }
            K::Property { receiver, property } => {
                self.u8(43);
                e(self, receiver);
                self.str(property);
            }
            K::MethodCall {
                receiver,
                method,
                args,
            } => {
                self.u8(44);
                e(self, receiver);
                self.str(method);
                self.exprs(args);
            }
            K::If { test, yes, no } => {
                self.u8(45);
                e(self, test);
                e(self, yes);
                e(self, no);
            }
            K::Do(items) => {
                self.u8(46);
                self.exprs(items);
            }
            K::Let { bindings, body } => {
                self.u8(47);
                self.bindings(bindings);
                e(self, body);
            }
            K::LetFn { bindings, body } => {
                self.u8(48);
                self.bindings(bindings);
                e(self, body);
            }
            K::Loop {
                target,
                bindings,
                body,
            } => {
                self.u8(49);
                self.index(target.0);
                self.bindings(bindings);
                e(self, body);
            }
            K::Recur { target, args } => {
                self.u8(50);
                self.index(target.0);
                self.exprs(args);
            }
            K::Throw(value) => {
                self.u8(51);
                e(self, value);
            }
            K::Await(value) => {
                self.u8(52);
                e(self, value);
            }
            K::Try {
                body,
                catches,
                finally,
            } => {
                self.u8(53);
                e(self, body);
                self.list(catches, |out, clause| {
                    let CatchClause {
                        class,
                        binding,
                        body,
                    } = clause;
                    out.option_index(class.map(|c| c.0));
                    out.index(binding.0);
                    out.index(body.0);
                });
                self.option_index(finally.map(|f| f.0));
            }
            K::Case {
                strict,
                keyword_tests,
                test,
                branches,
                default,
            } => {
                self.u8(54);
                self.bool(*strict);
                self.bool(*keyword_tests);
                e(self, test);
                self.list(branches, |out, (test, then)| {
                    out.index(test.0);
                    out.index(then.0);
                });
                self.option_index(default.map(|d| d.0));
            }
            K::Fn { name, methods } => {
                self.u8(55);
                self.option_index(name.map(|n| n.0));
                self.list(methods, Self::fn_method);
            }
            K::Call { callee, args } => {
                self.u8(56);
                e(self, callee);
                self.exprs(args);
            }
            K::InstanceOf { class, value } => {
                self.u8(57);
                e(self, class);
                e(self, value);
            }
        }
    }

    fn deferred_units(&mut self, units: &DeferredUnits) {
        let DeferredUnits {
            units,
            registrations,
        } = units;
        self.list(units, |out, unit| {
            let Unit {
                name,
                generation,
                analyses,
                emitted,
                code,
                view,
                extension_frontier,
            } = unit;
            match name {
                UnitName::Var(name) => {
                    out.u8(0);
                    out.str(name);
                }
                UnitName::Declaration => out.u8(1),
                UnitName::Test(name) => {
                    out.u8(2);
                    out.str(name);
                }
            }
            out.index(*generation);
            out.index(*analyses);
            out.bool(*emitted);
            out.code(code);
            out.view(view);
            out.index(*extension_frontier);
        });
        self.list(registrations, |out, registration| match registration {
            Registration::Root {
                name,
                unit,
                test,
                once,
                location,
            } => {
                out.u8(0);
                out.str(name);
                out.option_index(unit.map(|u| u.0));
                out.option_index(test.map(|u| u.0));
                out.bool(*once);
                out.str(location);
            }
            Registration::Bundle {
                products,
                unit,
                extensions,
                once,
                group,
                location,
            } => {
                out.u8(1);
                out.strs(products);
                out.index(unit.0);
                out.list(extensions, |out, extension| {
                    let Extension {
                        protocol,
                        selector,
                        identity,
                    } = extension;
                    out.str(protocol);
                    out.option_str(selector.as_deref());
                    match identity {
                        ExtensionIdentity::Method(offset) => {
                            out.u8(0);
                            out.index(*offset);
                        }
                        ExtensionIdentity::Text(text) => {
                            out.u8(1);
                            out.str(text);
                        }
                    }
                });
                out.bool(*once);
                out.bool(*group);
                out.str(location);
            }
        });
    }

    fn diagnostics(&mut self, diagnostics: &Diagnostics) {
        let Diagnostics {
            warnings,
            expansions,
        } = diagnostics;
        self.list(warnings, |out, warning| {
            let Warning {
                kind,
                message,
                span,
                source,
            } = warning;
            out.str(kind);
            out.str(message);
            out.bytes_range(*span);
            out.source(*source);
        });
        self.list(expansions, |out, expansion| {
            let MacroExpansion {
                macro_namespace,
                name,
                generation,
                call,
                source,
                definition,
            } = expansion;
            out.str(macro_namespace);
            out.str(name);
            out.index(*generation);
            out.bytes_range(*call);
            out.source(*source);
            out.site(definition);
        });
    }

    /// The analysis policy as one byte: the warning mode, the static
    /// array diagnostics, then the runtime array checks' tag.
    fn policy(&mut self, policy: AnalysisPolicy) {
        let AnalysisPolicy {
            warning_mode,
            checked_arrays,
            array_checks,
        } = policy;
        let checks = match array_checks {
            ArrayChecks::Off => 0,
            ArrayChecks::Warn => 1,
            ArrayChecks::Error => 2,
        };
        self.u8(u8::from(warning_mode) | u8::from(checked_arrays) << 1 | checks << 2);
    }
    fn names(&mut self, names: &GeneratedNames) {
        let GeneratedNames {
            from,
            to,
            gensym_to,
            gensyms,
        } = names;
        for counters in [from, to] {
            let Counters {
                syntax_quote,
                anonymous,
                binding,
                destructure,
            } = *counters;
            for value in [syntax_quote, anonymous, binding, destructure] {
                self.uint(value);
            }
        }
        self.uint(*gensym_to);
        self.list(gensyms, |out, units| out.units(units));
    }
}

fn collection_tag(kind: CollectionKind) -> u8 {
    match kind {
        CollectionKind::List => 0,
        CollectionKind::Vector => 1,
        CollectionKind::Map => 2,
        CollectionKind::Set => 3,
        CollectionKind::AnonymousFn => 4,
    }
}

fn literal_tag(kind: CompilerLiteralKind) -> u8 {
    match kind {
        CompilerLiteralKind::BigInteger => 0,
        CompilerLiteralKind::Regex => 1,
        CompilerLiteralKind::Instant => 2,
        CompilerLiteralKind::Uuid => 3,
        CompilerLiteralKind::JsArray => 4,
        CompilerLiteralKind::JsObject => 5,
        CompilerLiteralKind::Queue => 6,
    }
}

// ---------------------------------------------------------------------------
// Decoding

/// Read the manifest of encoded bytes, checking the header, the manifest's
/// integrity and the section table's layout, without decoding a section.
pub fn read_manifest(bytes: &[u8], limits: &DecodeLimits) -> Result<Manifest, DecodeError> {
    read_manifest_charged(
        bytes,
        limits,
        &Budget(std::cell::Cell::new(limits.max_decoded_bytes)),
    )
}

/// [`read_manifest`], charging what its external inputs allocate to
/// `budget`, which the sections' decoding then shares.
fn read_manifest_charged(
    bytes: &[u8],
    limits: &DecodeLimits,
    budget: &Budget,
) -> Result<Manifest, DecodeError> {
    if bytes.len() as u64 > limits.max_bytes {
        return Err(DecodeError::Limit {
            what: "bytes",
            value: bytes.len() as u64,
            limit: limits.max_bytes,
        });
    }
    if bytes.len() < MANIFEST_START {
        return Err(DecodeError::Truncated("header"));
    }
    if bytes[..8] != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let version = le32(&bytes[8..]);
    if version != SCHEMA_VERSION {
        return Err(DecodeError::Version(version));
    }
    if le32(&bytes[12..]) != BYTE_ORDER {
        return Err(DecodeError::ByteOrder);
    }
    let manifest_length = manifest_length(bytes)?;
    if manifest_hash(bytes, manifest_length)[..] != bytes[PREFIX..MANIFEST_START] {
        return Err(DecodeError::Integrity("manifest"));
    }
    let body = &bytes[MANIFEST_START..MANIFEST_START + manifest_length];
    let mut input = Input {
        bytes: body,
        position: 0,
        part: "manifest",
        limits,
    };
    let count = input.byte()? as usize;
    if count != Section::ALL.len() {
        return Err(DecodeError::Layout("wrong number of sections"));
    }
    let mut sections = Vec::with_capacity(count);
    let mut expected = (MANIFEST_START + manifest_length) as u64;
    for section in Section::ALL {
        let entry = input.take(SECTION_ENTRY)?;
        if entry[0] != section.tag() {
            return Err(DecodeError::Layout("sections out of order"));
        }
        let offset = le32(&entry[1..]);
        let length = le32(&entry[5..]);
        if u64::from(offset) != expected {
            return Err(DecodeError::Layout("sections are not contiguous"));
        }
        expected += u64::from(length);
        sections.push(SectionBounds {
            section,
            offset,
            length,
            sha256: entry[9..41].try_into().unwrap(),
        });
    }
    if expected > bytes.len() as u64 {
        return Err(DecodeError::Truncated("sections"));
    }
    if expected < bytes.len() as u64 {
        return Err(DecodeError::Trailing("sections"));
    }
    let namespace = input.utf8()?;
    let path = input.utf8()?;
    let source_length = input.u32()?;
    let identity = input.byte()?;
    if identity > 3 {
        return Err(DecodeError::Tag {
            what: "identity flags",
            tag: identity.into(),
        });
    }
    let front_end_digest = input.hash()?;
    let mut components = [[0; 32]; crate::cache_key::Component::ALL.len()];
    for component in &mut components {
        *component = input.hash()?;
    }
    let emission_key = input.hash()?;
    let core_table = input.hash()?;
    let names = |input: &mut Input<'_>| {
        let count = input.count()?;
        (0..count).map(|_| input.utf8()).collect::<Result<_, _>>()
    };
    let requires = names(&mut input)?;
    let references = names(&mut input)?;
    let count = input.count()?;
    let mut dependencies = Vec::with_capacity(count);
    for _ in 0..count {
        dependencies.push(Dependency {
            namespace: input.utf8()?,
            kinds: input.byte()?,
        });
    }
    let count = input.count()?;
    let mut key_dependencies = budget.vec(count)?;
    for _ in 0..count {
        key_dependencies.push(crate::cache_key::DependencyKey {
            namespace: input.utf8_charged(budget)?,
            kinds: input.byte()?,
            key: input.hash()?,
        });
    }
    let inputs = read_external_inputs(&mut input, budget)?;
    let policy = input.byte()?;
    if policy > 7 {
        return Err(DecodeError::Tag {
            what: "policy flags",
            tag: policy.into(),
        });
    }
    input.end()?;
    Ok(Manifest {
        namespace,
        path,
        source_length,
        cljc: identity & 1 != 0,
        entry: identity & 2 != 0,
        core_table,
        requires,
        references,
        fields: ManifestFields {
            front_end_key: crate::cache_key::FrontEndKey {
                digest: front_end_digest,
                components,
                dependencies: key_dependencies,
            },
            emission_key,
            dependencies,
            inputs,
            coarse: policy & 1 != 0,
            no_cache: policy & 2 != 0,
            always: policy & 4 != 0,
        },
        sections,
        length: bytes.len() as u64,
    })
}

/// Decode encoded bytes into an owned artifact: the manifest, each
/// section's integrity, then the sections, polling `cancelled` through long
/// tables, and finally [`NamespaceArtifact::validate`].
pub fn decode(
    bytes: &[u8],
    limits: &DecodeLimits,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Manifest, NamespaceArtifact), DecodeError> {
    if cancelled() {
        return Err(DecodeError::Cancelled);
    }
    let budget = Budget(std::cell::Cell::new(limits.max_decoded_bytes));
    let manifest = read_manifest_charged(bytes, limits, &budget)?;
    if manifest.core_table != core_table() {
        return Err(DecodeError::CoreTable);
    }
    let section = |index: usize| {
        let bounds = &manifest.sections[index];
        let start = bounds.offset as usize;
        let content = &bytes[start..start + bounds.length as usize];
        if polled_digest(content, cancelled)? != bounds.sha256 {
            return Err(DecodeError::Integrity(bounds.section.name()));
        }
        Ok(content)
    };
    let mut strings_input = Input {
        bytes: section(0)?,
        position: 0,
        part: "strings",
        limits,
    };
    let strings = strings_input.string_table(cancelled, &budget)?;
    let reader = |index: usize| -> Result<Reader<'_>, DecodeError> {
        Ok(Reader {
            input: Input {
                bytes: section(index)?,
                position: 0,
                part: Section::ALL[index].name(),
                limits,
            },
            strings: &strings,
            cancelled,
            polled: 0,
            polled_at: 0,
            budget: &budget,
        })
    };

    let mut r = reader(1)?;
    let source_name = r.option_string()?;
    let root = ExprRef(r.index()?);
    let policy = match r.input.byte()? {
        flags if flags >> 2 <= 2 => AnalysisPolicy {
            warning_mode: flags & 1 != 0,
            checked_arrays: flags & 2 != 0,
            array_checks: match flags >> 2 {
                0 => ArrayChecks::Off,
                1 => ArrayChecks::Warn,
                _ => ArrayChecks::Error,
            },
        },
        tag => return Err(r.tag("analysis policy", tag)),
    };
    let mut declarations = r.declarations()?;
    declarations.requires = manifest.requires.clone();
    let patches = r.list(|r| {
        Ok(GlobalPatch {
            global: r.global()?,
            flags: r.flags()?,
            metadata: r.option_index()?.map(ExprRef),
        })
    })?;
    let globals = r.list(|r| {
        Ok(ExternalGlobal {
            namespace: r.string()?,
            name: r.string()?,
            declared: r.bool()?,
            private: r.option_bool()?,
        })
    })?;
    r.input.end()?;

    let mut r = reader(2)?;
    let ir = r.ir()?;
    r.input.end()?;

    let mut r = reader(3)?;
    let units = r.deferred_units()?;
    r.input.end()?;

    let mut r = reader(4)?;
    let diagnostics = r.diagnostics()?;
    r.input.end()?;

    let mut r = reader(5)?;
    let names = r.names()?;
    let accounting = match r.input.byte()? {
        0 => None,
        1 => Some(Accounting {
            expansions: r.uint()?,
            generated_nodes: r.uint()?,
            node_peak: r.uint()?,
            macro_depth: r.index()?,
            analysis_depth: r.index()?,
            unit_depth: r.index()?,
            conversion_peak: r.uint()?,
        }),
        tag => return Err(r.tag("accounting", tag)),
    };
    r.input.end()?;

    let artifact = NamespaceArtifact {
        header: Header {
            namespace: manifest.namespace.clone(),
            path: manifest.path.clone(),
            source_length: manifest.source_length,
            cljc: manifest.cljc,
            source_name,
            entry: manifest.entry,
            root,
            policy,
        },
        declarations,
        ir,
        patches,
        units,
        diagnostics,
        references: References {
            namespaces: manifest.references.clone(),
            globals,
        },
        names,
        accounting,
    };
    if cancelled() {
        return Err(DecodeError::Cancelled);
    }
    artifact.validate().map_err(DecodeError::Invalid)?;
    let depth = artifact.ir_depth().map_err(DecodeError::Invalid)?;
    if depth > limits.max_ir_depth {
        return Err(DecodeError::Limit {
            what: "expression nesting",
            value: depth.into(),
            limit: limits.max_ir_depth.into(),
        });
    }
    Ok((manifest, artifact))
}

/// How many bytes decoding hashes, reads or copies between cancellation
/// polls, at most.
const POLL_BYTES: usize = 1 << 20;

/// The SHA-256 of `content`, polling for cancellation between pieces.
fn polled_digest(content: &[u8], cancelled: &dyn Fn() -> bool) -> Result<[u8; 32], DecodeError> {
    let mut hash = Sha256::new();
    for piece in content.chunks(POLL_BYTES) {
        if cancelled() {
            return Err(DecodeError::Cancelled);
        }
        hash.update(piece);
    }
    Ok(hash.finish())
}

/// What decoding may still allocate ([`DecodeLimits::max_decoded_bytes`]).
struct Budget(std::cell::Cell<u64>);

impl Budget {
    fn charge(&self, bytes: u64) -> Result<(), DecodeError> {
        match self.0.get().checked_sub(bytes) {
            Some(left) => {
                self.0.set(left);
                Ok(())
            }
            None => Err(DecodeError::Limit {
                what: "decoded bytes",
                value: bytes,
                limit: self.0.get(),
            }),
        }
    }

    /// An empty vector with room for `count` entries, charged first.
    fn vec<T>(&self, count: usize) -> Result<Vec<T>, DecodeError> {
        self.charge((count as u64).saturating_mul(std::mem::size_of::<T>() as u64))?;
        let mut items = Vec::new();
        items
            .try_reserve_exact(count)
            .map_err(|_| DecodeError::Limit {
                what: "decoded bytes",
                value: count as u64,
                limit: self.0.get(),
            })?;
        Ok(items)
    }
}

/// Bounded reading of one part of the bytes.
struct Input<'a> {
    bytes: &'a [u8],
    position: usize,
    part: &'static str,
    limits: &'a DecodeLimits,
}

impl<'a> Input<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], DecodeError> {
        let end = self
            .position
            .checked_add(length)
            .filter(|&end| end <= self.bytes.len())
            .ok_or(DecodeError::Truncated(self.part))?;
        let taken = &self.bytes[self.position..end];
        self.position = end;
        Ok(taken)
    }
    fn byte(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
    fn uint(&mut self) -> Result<u64, DecodeError> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            let bits = u64::from(byte & 0x7f);
            if shift == 63 && bits > 1 {
                return Err(DecodeError::Overflow(self.part));
            }
            value |= bits << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(DecodeError::Overflow(self.part))
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        u32::try_from(self.uint()?).map_err(|_| DecodeError::Overflow(self.part))
    }
    /// A count of entries that each take at least one byte.
    fn count(&mut self) -> Result<usize, DecodeError> {
        let count = self.uint()?;
        if count > self.limits.max_entries {
            return Err(DecodeError::Limit {
                what: "entries",
                value: count,
                limit: self.limits.max_entries,
            });
        }
        if count > self.remaining() as u64 {
            return Err(DecodeError::Truncated(self.part));
        }
        Ok(count as usize)
    }
    fn hash(&mut self) -> Result<[u8; 32], DecodeError> {
        Ok(self.take(32)?.try_into().unwrap())
    }
    /// [`Self::utf8`], its bytes charged to `budget` before they are copied.
    fn utf8_charged(&mut self, budget: &Budget) -> Result<String, DecodeError> {
        let start = self.position;
        let length = self.uint()?;
        if length > self.remaining() as u64 {
            return Err(DecodeError::Truncated(self.part));
        }
        budget.charge(length)?;
        self.position = start;
        self.utf8()
    }
    fn utf8(&mut self) -> Result<String, DecodeError> {
        let length = self.uint()?;
        if length > self.remaining() as u64 {
            return Err(DecodeError::Truncated(self.part));
        }
        let bytes = self.take(length as usize)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| DecodeError::Text(self.part))
    }
    fn end(&self) -> Result<(), DecodeError> {
        if self.position == self.bytes.len() {
            Ok(())
        } else {
            Err(DecodeError::Trailing(self.part))
        }
    }
    fn string_table(
        &mut self,
        cancelled: &dyn Fn() -> bool,
        budget: &Budget,
    ) -> Result<Vec<Vec<u16>>, DecodeError> {
        let count = self.count()?;
        let mut table = budget.vec(count)?;
        let mut polled_at = 0;
        for index in 0..count {
            if ((index + 1).is_multiple_of(POLL) || self.position - polled_at >= POLL_BYTES)
                && cancelled()
            {
                return Err(DecodeError::Cancelled);
            }
            let length = self.uint()?;
            if length > self.limits.max_string {
                return Err(DecodeError::Limit {
                    what: "string code units",
                    value: length,
                    limit: self.limits.max_string,
                });
            }
            polled_at = self.position;
            budget.charge(length.saturating_mul(2))?;
            let bytes = self.take(
                usize::try_from(length.saturating_mul(2))
                    .map_err(|_| DecodeError::Truncated(self.part))?,
            )?;
            table.push(
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_le_bytes(*pair))
                    .collect(),
            );
        }
        self.end()?;
        Ok(table)
    }
}

/// How many entries of a table decode between cancellation polls.
const POLL: usize = 1024;

struct Reader<'a> {
    input: Input<'a>,
    strings: &'a [Vec<u16>],
    cancelled: &'a dyn Fn() -> bool,
    polled: usize,
    /// The input position at the last poll.
    polled_at: usize,
    budget: &'a Budget,
}

impl<'a> Reader<'a> {
    fn tag(&self, what: &'static str, tag: u8) -> DecodeError {
        DecodeError::Tag {
            what,
            tag: tag.into(),
        }
    }
    fn poll(&mut self) -> Result<(), DecodeError> {
        self.polled += 1;
        if self.polled.is_multiple_of(POLL) || self.input.position - self.polled_at >= POLL_BYTES {
            self.polled_at = self.input.position;
            if (self.cancelled)() {
                return Err(DecodeError::Cancelled);
            }
        }
        Ok(())
    }
    fn byte(&mut self) -> Result<u8, DecodeError> {
        self.input.byte()
    }
    fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(self.tag("boolean", tag)),
        }
    }
    fn uint(&mut self) -> Result<u64, DecodeError> {
        self.input.uint()
    }
    fn index(&mut self) -> Result<u32, DecodeError> {
        self.input.u32()
    }
    fn int(&mut self) -> Result<i64, DecodeError> {
        let value = self.uint()?;
        Ok((value >> 1) as i64 ^ -((value & 1) as i64))
    }
    fn f64(&mut self) -> Result<f64, DecodeError> {
        let bytes = self.input.take(8)?;
        Ok(f64::from_bits(u64::from_le_bytes(
            bytes.try_into().unwrap(),
        )))
    }
    fn option_index(&mut self) -> Result<Option<u32>, DecodeError> {
        match self.uint()? {
            0 => Ok(None),
            value => u32::try_from(value - 1)
                .map(Some)
                .map_err(|_| DecodeError::Overflow(self.input.part)),
        }
    }
    fn option_bool(&mut self) -> Result<Option<bool>, DecodeError> {
        match self.byte()? {
            0 => Ok(None),
            1 => Ok(Some(false)),
            2 => Ok(Some(true)),
            tag => Err(self.tag("optional boolean", tag)),
        }
    }
    fn present(&mut self, what: &'static str) -> Result<bool, DecodeError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(self.tag(what, tag)),
        }
    }
    fn text(&mut self) -> Result<&'a [u16], DecodeError> {
        let index = self.uint()?;
        self.strings
            .get(usize::try_from(index).unwrap_or(usize::MAX))
            .map(Vec::as_slice)
            .ok_or(DecodeError::Index {
                what: "string",
                index,
                len: self.strings.len() as u64,
            })
    }
    /// Charge a copy of `units` code units at `per_unit` bytes each,
    /// polling first for a long one.
    fn copying(&self, units: usize, per_unit: u64) -> Result<(), DecodeError> {
        if units >= POLL_BYTES / 2 && (self.cancelled)() {
            return Err(DecodeError::Cancelled);
        }
        self.budget.charge((units as u64).saturating_mul(per_unit))
    }
    fn units(&mut self) -> Result<Vec<u16>, DecodeError> {
        let text = self.text()?;
        self.copying(text.len(), 2)?;
        Ok(text.to_vec())
    }
    fn string(&mut self) -> Result<String, DecodeError> {
        let part = self.input.part;
        let text = self.text()?;
        // At most three UTF-8 bytes for each code unit.
        self.copying(text.len(), 3)?;
        String::from_utf16(text).map_err(|_| DecodeError::Text(part))
    }
    fn option_string(&mut self) -> Result<Option<String>, DecodeError> {
        if self.present("optional text")? {
            self.string().map(Some)
        } else {
            Ok(None)
        }
    }
    fn option_units(&mut self) -> Result<Option<Vec<u16>>, DecodeError> {
        if self.present("optional text")? {
            self.units().map(Some)
        } else {
            Ok(None)
        }
    }
    fn list<T>(
        &mut self,
        mut each: impl FnMut(&mut Self) -> Result<T, DecodeError>,
    ) -> Result<Vec<T>, DecodeError> {
        let count = self.input.count()?;
        let mut items = self.budget.vec(count)?;
        for _ in 0..count {
            self.poll()?;
            items.push(each(self)?);
        }
        Ok(items)
    }
    fn strings(&mut self) -> Result<Vec<String>, DecodeError> {
        self.list(Self::string)
    }
    fn pairs(&mut self) -> Result<Vec<(String, String)>, DecodeError> {
        self.list(|r| Ok((r.string()?, r.string()?)))
    }
    fn triples(&mut self) -> Result<Vec<(String, String, String)>, DecodeError> {
        self.list(|r| Ok((r.string()?, r.string()?, r.string()?)))
    }
    fn bytes_range(&mut self) -> Result<Bytes, DecodeError> {
        Ok((self.index()?, self.index()?))
    }
    fn source(&mut self) -> Result<SourceRef, DecodeError> {
        Ok(match self.option_index()? {
            None => SourceRef::Own,
            Some(index) => SourceRef::Namespace(index),
        })
    }
    fn global(&mut self) -> Result<GlobalRef, DecodeError> {
        let value = self.uint()?;
        let index =
            u32::try_from(value >> 1).map_err(|_| DecodeError::Overflow(self.input.part))?;
        Ok(if value & 1 == 0 {
            GlobalRef::Own(index)
        } else {
            GlobalRef::External(index)
        })
    }
    fn option_global(&mut self) -> Result<Option<GlobalRef>, DecodeError> {
        if self.present("optional global")? {
            self.global().map(Some)
        } else {
            Ok(None)
        }
    }
    fn expr(&mut self) -> Result<ExprRef, DecodeError> {
        self.index().map(ExprRef)
    }
    fn exprs(&mut self) -> Result<Vec<ExprRef>, DecodeError> {
        self.list(Self::expr)
    }
    fn local(&mut self) -> Result<LocalRef, DecodeError> {
        self.index().map(LocalRef)
    }
    fn locals(&mut self) -> Result<Vec<LocalRef>, DecodeError> {
        self.list(Self::local)
    }
    fn flags(&mut self) -> Result<GlobalFlags, DecodeError> {
        let bits = self.byte()?;
        if bits >> 5 != 0 {
            return Err(self.tag("global flags", bits));
        }
        Ok(GlobalFlags {
            type_descriptor: bits & 1 != 0,
            declared: bits & 2 != 0,
            constant: bits & 4 != 0,
            define: bits & 8 != 0,
            boolean_tag: bits & 16 != 0,
        })
    }
    fn site(&mut self) -> Result<Site, DecodeError> {
        Ok(Site {
            source: self.string()?,
            bytes: self.bytes_range()?,
        })
    }
    fn code(&mut self) -> Result<RelocatableCode, DecodeError> {
        Ok(RelocatableCode {
            text: self.string()?,
            units: self.list(|r| Ok((r.bytes_range()?, UnitRef(r.index()?))))?,
        })
    }
    fn reader_context(&mut self) -> Result<ReaderContext, DecodeError> {
        Ok(ReaderContext {
            aliases: self.pairs()?,
            refers: self.triples()?,
            excludes: self.strings()?,
            macros: self.strings()?,
        })
    }
    fn view(&mut self) -> Result<RegistrationView, DecodeError> {
        Ok(RegistrationView {
            own_globals: self.index()?,
            added: self.list(Self::global)?,
            reader_context: if self.present("reader context")? {
                Some(self.reader_context()?)
            } else {
                None
            },
        })
    }
    fn protocol_method(&mut self) -> Result<ProtocolMethod, DecodeError> {
        Ok(ProtocolMethod {
            global: self.global()?,
            key: self.string()?,
            arities: self.list(Self::index)?,
            variadic: self.option_index()?,
        })
    }
    fn datum(&mut self, depth: u32) -> Result<Datum, DecodeError> {
        let limit = self.input.limits.max_depth;
        if depth >= limit {
            return Err(DecodeError::TooDeep(limit));
        }
        let kind = match self.byte()? {
            0 => DatumKind::Nil,
            1 => DatumKind::Boolean(self.bool()?),
            2 => DatumKind::Integer(self.int()?),
            3 => DatumKind::Double(self.f64()?),
            4 => DatumKind::Ratio {
                numerator: self.int()?,
                denominator: self.int()?,
            },
            5 => DatumKind::Character(
                u16::try_from(self.uint()?).map_err(|_| DecodeError::Overflow(self.input.part))?,
            ),
            6 => DatumKind::String(self.units()?),
            7 => DatumKind::Symbol(self.option_units()?, self.units()?),
            8 => DatumKind::Keyword(self.option_units()?, self.units()?),
            9 => DatumKind::Collection {
                kind: self.collection_kind()?,
                children: self.list(|r| r.datum(depth + 1))?,
            },
            10 => DatumKind::CompilerLiteral {
                kind: self.literal_kind()?,
                payload: self.boxed_datum(depth + 1)?,
            },
            tag => return Err(self.tag("datum", tag)),
        };
        let metadata = if self.present("datum metadata")? {
            Some(self.boxed_datum(depth + 1)?)
        } else {
            None
        };
        Ok(Datum { kind, metadata })
    }
    fn boxed_datum(&mut self, depth: u32) -> Result<Box<Datum>, DecodeError> {
        self.budget.charge(std::mem::size_of::<Datum>() as u64)?;
        Ok(Box::new(self.datum(depth)?))
    }
    fn option_datum(&mut self) -> Result<Option<Datum>, DecodeError> {
        if self.present("optional datum")? {
            self.datum(0).map(Some)
        } else {
            Ok(None)
        }
    }
    fn collection_kind(&mut self) -> Result<CollectionKind, DecodeError> {
        Ok(match self.byte()? {
            0 => CollectionKind::List,
            1 => CollectionKind::Vector,
            2 => CollectionKind::Map,
            3 => CollectionKind::Set,
            4 => CollectionKind::AnonymousFn,
            tag => return Err(self.tag("collection kind", tag)),
        })
    }
    fn literal_kind(&mut self) -> Result<CompilerLiteralKind, DecodeError> {
        Ok(match self.byte()? {
            0 => CompilerLiteralKind::BigInteger,
            1 => CompilerLiteralKind::Regex,
            2 => CompilerLiteralKind::Instant,
            3 => CompilerLiteralKind::Uuid,
            4 => CompilerLiteralKind::JsArray,
            5 => CompilerLiteralKind::JsObject,
            6 => CompilerLiteralKind::Queue,
            tag => return Err(self.tag("compiler literal kind", tag)),
        })
    }

    fn declarations(&mut self) -> Result<Declarations, DecodeError> {
        let globals = self.list(|r| {
            Ok(Global {
                namespace: r.string()?,
                name: r.string()?,
                flags: r.flags()?,
                metadata: r.option_index()?.map(ExprRef),
                span: r.bytes_range()?,
                source: r.source()?,
                private: r.option_bool()?,
                binding: match r.byte()? {
                    0 => None,
                    1 => Some(BindingKind::Value),
                    2 => Some(BindingKind::Macro),
                    tag => return Err(r.tag("binding kind", tag)),
                },
            })
        })?;
        let protocols = self.list(|r| Ok((r.global()?, r.list(Self::protocol_method)?)))?;
        let record_constructors = self.list(Self::global)?;
        let field_bindings =
            self.list(|r| Ok((LocalRef(r.index()?), RecurRef(r.index()?), r.bool()?)))?;
        let type_methods = self.list(|r| r.index().map(RecurRef))?;
        let macro_functions = self.exprs()?;
        let macros = self.list(|r| {
            Ok(MacroDeclaration {
                name: r.string()?,
                generation: r.index()?,
                metadata: r.datum(0)?,
                definition: r.site()?,
                code: r.code()?,
                view: r.view()?,
            })
        })?;
        let aliases = self.pairs()?;
        let refers = self.triples()?;
        let excludes = self.strings()?;
        let js_refers = self.list(|r| {
            let local = r.string()?;
            let refer = match r.byte()? {
                0 => JsRefer::Global(r.string()?),
                1 => JsRefer::Package(r.string()?, r.option_string()?),
                tag => return Err(r.tag("host refer", tag)),
            };
            Ok((local, refer))
        })?;
        let reader_context = self.reader_context()?;
        let imports = self.list(|r| {
            Ok(Import {
                namespace: r.string()?,
                alias: r.option_string()?,
                refers: r.strings()?,
            })
        })?;
        let renames = self.triples()?;
        let namespace_imports = self.pairs()?;
        let namespace_symbol = self.option_datum()?;
        let namespace_doc = self.option_datum()?;
        let lazy_targets = self.list(|r| {
            Ok(LazyTarget {
                site: r.expr()?,
                namespace: r.string()?,
                global: r.option_global()?,
            })
        })?;
        Ok(Declarations {
            globals,
            protocols,
            record_constructors,
            field_bindings,
            type_methods,
            macro_functions,
            macros,
            aliases,
            requires: Vec::new(),
            refers,
            excludes,
            js_refers,
            reader_context,
            imports,
            renames,
            namespace_imports,
            namespace_symbol,
            namespace_doc,
            lazy_targets,
        })
    }

    fn ir(&mut self) -> Result<SemanticIr, DecodeError> {
        let strings = self.list(Self::units)?;
        let named_literals = self.list(|r| {
            Ok(NamedLiteral {
                namespace: r.option_index()?.map(StringRef),
                name: StringRef(r.index()?),
            })
        })?;
        let js_globals = self.list(Self::strings)?;
        let locals = self.list(|r| {
            Ok(LocalBinding {
                name: r.string()?,
                span: r.bytes_range()?,
                source: r.source()?,
            })
        })?;
        let recur_targets = self.list(|r| {
            let params = r.locals()?;
            let bits = r.byte()?;
            if bits >> 3 != 0 {
                return Err(r.tag("recur target flags", bits));
            }
            Ok(RecurTarget {
                params,
                protocol_impl: bits & 1 != 0,
                receiver_from_this: bits & 2 != 0,
                used: bits & 4 != 0,
                argument_slots: r.list(Self::option_index)?,
            })
        })?;
        let exprs = self.list(|r| {
            let context = match r.byte()? {
                0 => ExprContext::Value,
                1 => ExprContext::Discard,
                2 => ExprContext::Return,
                tag => return Err(r.tag("expression context", tag)),
            };
            Ok(Expr {
                span: r.bytes_range()?,
                source: r.source()?,
                kind: r.kind()?,
                context,
            })
        })?;
        Ok(SemanticIr {
            exprs,
            locals,
            recur_targets,
            strings,
            named_literals,
            js_globals,
        })
    }

    fn fn_method(&mut self) -> Result<FnMethod, DecodeError> {
        Ok(FnMethod {
            params: self.locals()?,
            rest: self.option_index()?.map(LocalRef),
            body: self.expr()?,
            target: RecurRef(self.index()?),
            captures: self.locals()?,
            asynchronous: self.bool()?,
        })
    }

    fn methods(&mut self) -> Result<Vec<(MethodDispatch, FnMethod)>, DecodeError> {
        self.list(|r| {
            let dispatch = match r.byte()? {
                0 => MethodDispatch::Protocol(r.string()?),
                1 => MethodDispatch::Builtin(r.string()?),
                tag => return Err(r.tag("method dispatch", tag)),
            };
            Ok((dispatch, r.fn_method()?))
        })
    }

    fn bindings(&mut self) -> Result<Vec<(LocalRef, ExprRef)>, DecodeError> {
        self.list(|r| Ok((r.local()?, r.expr()?)))
    }

    fn kind(&mut self) -> Result<ExprKind, DecodeError> {
        use ExprKind as K;
        Ok(match self.byte()? {
            0 => K::Nil,
            1 => K::Bool(self.bool()?),
            2 => K::Number(self.f64()?),
            3 => K::String(StringRef(self.index()?)),
            4 => K::ReaderLiteral {
                kind: self.literal_kind()?,
                items: self.exprs()?,
                quoted: self.bool()?,
            },
            5 => K::DefinitionTest {
                value: self.expr()?,
                unit: UnitRef(self.index()?),
            },
            6 => K::WithMeta {
                value: self.expr()?,
                metadata: self.expr()?,
            },
            7 => K::Var {
                definition: self.bool()?,
                value: self.expr()?,
                symbol: NamedRef(self.index()?),
                metadata: match self.byte()? {
                    0 => None,
                    1 => Some(VarMetadata::Expr(self.expr()?)),
                    2 => Some(VarMetadata::Global(self.global()?)),
                    tag => return Err(self.tag("var metadata", tag)),
                },
            },
            8 => K::Keyword(NamedRef(self.index()?)),
            9 => K::Symbol(NamedRef(self.index()?)),
            10 => K::List(self.exprs()?),
            11 => K::Vector(self.exprs()?),
            12 => K::Map(self.exprs()?),
            13 => K::Set(self.exprs()?),
            14 => K::LiteralMap(self.exprs()?),
            15 => K::LiteralSet(self.exprs()?),
            16 => {
                let row = self.uint()?;
                K::CoreFn(
                    *CoreFnId::ALL
                        .get(usize::try_from(row).unwrap_or(usize::MAX))
                        .ok_or(DecodeError::Index {
                            what: "core operation",
                            index: row,
                            len: CoreFnId::ALL.len() as u64,
                        })?,
                )
            }
            17 => K::Local(self.local()?),
            18 => K::Global(self.global()?),
            19 => K::JsGlobal(JsGlobalRef(self.index()?)),
            20 => K::ThisAs {
                binding: self.local()?,
                body: self.expr()?,
            },
            21 => K::JsArguments,
            22 => K::JsTemplate {
                segments: self.strings()?,
                args: self.exprs()?,
            },
            23 => K::JsIndex {
                receiver: self.expr()?,
                indices: self.exprs()?,
            },
            24 => K::JsDelete {
                receiver: self.expr()?,
                key: self.expr()?,
            },
            25 => K::QualifiedMethod {
                class: self.expr()?,
                method: self.option_string()?,
            },
            26 => K::CopyArguments(self.expr()?),
            27 => K::DefProtocol {
                global: self.global()?,
                methods: self.list(Self::protocol_method)?,
                via_metadata: self.bool()?,
            },
            28 => K::ProtocolPredicate {
                protocol: self.expr()?,
                value: self.expr()?,
                key: self.string()?,
                direct: self.bool()?,
            },
            29 => K::ExtendType {
                instance: self.bool()?,
                return_target: self.bool()?,
                target: self.expr()?,
                methods: self.methods()?,
            },
            30 => K::Reify {
                metadata: self.option_index()?.map(ExprRef),
                captures: self.locals()?,
                methods: self.methods()?,
            },
            31 => K::DefType {
                record: self.bool()?,
                global: self.global()?,
                methods: self.methods()?,
                layout: {
                    self.budget
                        .charge(std::mem::size_of::<TypeLayout>() as u64)?;
                    Box::new(TypeLayout {
                        constructor: self.option_global()?,
                        body: self.option_index()?.map(ExprRef),
                        masks: self.option_index()?.map(ExprRef),
                        map_constructor: self.option_global()?,
                        fields: self.strings()?,
                    })
                },
            },
            32 => K::Construct {
                callee: self.expr()?,
                args: self.exprs()?,
            },
            33 => K::Assert {
                test: self.expr()?,
                message: self.option_index()?.map(ExprRef),
                form: StringRef(self.index()?),
            },
            34 => K::Def {
                global: self.global()?,
                init: self.option_index()?.map(ExprRef),
                test: self.option_index()?.map(ExprRef),
                once: self.bool()?,
            },
            35 => K::SetGlobal {
                target: self.expr()?,
                value: self.expr()?,
            },
            36 => K::Binding {
                bindings: self.list(|r| Ok((r.expr()?, r.expr()?)))?,
                body: self.expr()?,
            },
            37 => K::WithOutStr(self.expr()?),
            38 => K::Exists(self.expr()?),
            39 => K::Loadable {
                target: LazyRef(self.index()?),
                class: self.expr()?,
            },
            40 => K::LoadableType,
            41 => K::BindingExists(self.expr()?),
            42 => K::BooleanHint(self.expr()?),
            43 => K::Property {
                receiver: self.expr()?,
                property: self.string()?,
            },
            44 => K::MethodCall {
                receiver: self.expr()?,
                method: self.string()?,
                args: self.exprs()?,
            },
            45 => K::If {
                test: self.expr()?,
                yes: self.expr()?,
                no: self.expr()?,
            },
            46 => K::Do(self.exprs()?),
            47 => K::Let {
                bindings: self.bindings()?,
                body: self.expr()?,
            },
            48 => K::LetFn {
                bindings: self.bindings()?,
                body: self.expr()?,
            },
            49 => K::Loop {
                target: RecurRef(self.index()?),
                bindings: self.bindings()?,
                body: self.expr()?,
            },
            50 => K::Recur {
                target: RecurRef(self.index()?),
                args: self.exprs()?,
            },
            51 => K::Throw(self.expr()?),
            52 => K::Await(self.expr()?),
            53 => K::Try {
                body: self.expr()?,
                catches: self.list(|r| {
                    Ok(CatchClause {
                        class: r.option_index()?.map(ExprRef),
                        binding: r.local()?,
                        body: r.expr()?,
                    })
                })?,
                finally: self.option_index()?.map(ExprRef),
            },
            54 => K::Case {
                strict: self.bool()?,
                keyword_tests: self.bool()?,
                test: self.expr()?,
                branches: self.list(|r| Ok((r.expr()?, r.expr()?)))?,
                default: self.option_index()?.map(ExprRef),
            },
            55 => K::Fn {
                name: self.option_index()?.map(LocalRef),
                methods: self.list(Self::fn_method)?,
            },
            56 => K::Call {
                callee: self.expr()?,
                args: self.exprs()?,
            },
            57 => K::InstanceOf {
                class: self.expr()?,
                value: self.expr()?,
            },
            tag => return Err(self.tag("expression", tag)),
        })
    }

    fn deferred_units(&mut self) -> Result<DeferredUnits, DecodeError> {
        let units = self.list(|r| {
            let name = match r.byte()? {
                0 => UnitName::Var(r.string()?),
                1 => UnitName::Declaration,
                2 => UnitName::Test(r.string()?),
                tag => return Err(r.tag("unit name", tag)),
            };
            Ok(Unit {
                name,
                generation: r.index()?,
                analyses: r.index()?,
                emitted: r.bool()?,
                code: r.code()?,
                view: r.view()?,
                extension_frontier: r.index()?,
            })
        })?;
        let registrations = self.list(|r| {
            Ok(match r.byte()? {
                0 => Registration::Root {
                    name: r.string()?,
                    unit: r.option_index()?.map(UnitRef),
                    test: r.option_index()?.map(UnitRef),
                    once: r.bool()?,
                    location: r.string()?,
                },
                1 => Registration::Bundle {
                    products: r.strings()?,
                    unit: UnitRef(r.index()?),
                    extensions: r.list(|r| {
                        Ok(Extension {
                            protocol: r.string()?,
                            selector: r.option_string()?,
                            identity: match r.byte()? {
                                0 => ExtensionIdentity::Method(r.index()?),
                                1 => ExtensionIdentity::Text(r.string()?),
                                tag => return Err(r.tag("extension identity", tag)),
                            },
                        })
                    })?,
                    once: r.bool()?,
                    group: r.bool()?,
                    location: r.string()?,
                },
                tag => return Err(r.tag("registration", tag)),
            })
        })?;
        Ok(DeferredUnits {
            units,
            registrations,
        })
    }

    fn diagnostics(&mut self) -> Result<Diagnostics, DecodeError> {
        Ok(Diagnostics {
            warnings: self.list(|r| {
                Ok(Warning {
                    kind: r.string()?,
                    message: r.string()?,
                    span: r.bytes_range()?,
                    source: r.source()?,
                })
            })?,
            expansions: self.list(|r| {
                Ok(MacroExpansion {
                    macro_namespace: r.string()?,
                    name: r.string()?,
                    generation: r.index()?,
                    call: r.bytes_range()?,
                    source: r.source()?,
                    definition: r.site()?,
                })
            })?,
        })
    }

    fn names(&mut self) -> Result<GeneratedNames, DecodeError> {
        let mut counters = || -> Result<Counters, DecodeError> {
            Ok(Counters {
                syntax_quote: self.uint()?,
                anonymous: self.uint()?,
                binding: self.uint()?,
                destructure: self.uint()?,
            })
        };
        let from = counters()?;
        let to = counters()?;
        Ok(GeneratedNames {
            from,
            to,
            gensym_to: self.uint()?,
            gensyms: self.list(Self::units)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependency_manifest::{
        BroadView, Candidate, ExternalInput, ResolvedResource, ResourceDependency,
    };

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn datum(kind: DatumKind) -> Datum {
        Datum {
            kind,
            metadata: None,
        }
    }

    /// Reader data of every kind.
    fn every_datum() -> Datum {
        let mut tagged = datum(DatumKind::Keyword(None, units("tagged")));
        tagged.metadata = Some(Box::new(datum(DatumKind::Boolean(true))));
        datum(DatumKind::Collection {
            kind: CollectionKind::Map,
            children: vec![
                datum(DatumKind::Nil),
                datum(DatumKind::Boolean(false)),
                datum(DatumKind::Integer(i64::MIN)),
                datum(DatumKind::Integer(i64::MAX)),
                datum(DatumKind::Double(-0.0)),
                datum(DatumKind::Ratio {
                    numerator: -3,
                    denominator: 7,
                }),
                datum(DatumKind::Character(0xffff)),
                // A lone surrogate survives as reader text.
                datum(DatumKind::String(vec![0xd800, 0x61])),
                datum(DatumKind::Symbol(Some(units("app.core")), units("f"))),
                tagged,
                datum(DatumKind::Collection {
                    kind: CollectionKind::AnonymousFn,
                    children: vec![datum(DatumKind::Collection {
                        kind: CollectionKind::Set,
                        children: Vec::new(),
                    })],
                }),
                datum(DatumKind::CompilerLiteral {
                    kind: CompilerLiteralKind::BigInteger,
                    payload: Box::new(datum(DatumKind::String(units(
                        "123456789012345678901234567890",
                    )))),
                }),
            ],
        })
    }

    /// Every expression kind once, referring to entry 0 of each table.
    fn every_kind() -> Vec<ExprKind> {
        use ExprKind as K;
        let e = ExprRef(0);
        let l = LocalRef(0);
        let method = FnMethod {
            params: vec![l],
            rest: Some(LocalRef(1)),
            body: e,
            target: RecurRef(0),
            captures: vec![LocalRef(1)],
            asynchronous: true,
        };
        let protocol_method = ProtocolMethod {
            global: GlobalRef::Own(0),
            key: "app$core$P$m$arity$1".into(),
            arities: vec![1, 2],
            variadic: Some(3),
        };
        let methods = vec![
            (
                MethodDispatch::Protocol("app.core/P".into()),
                method.clone(),
            ),
            (MethodDispatch::Builtin("toString".into()), method.clone()),
        ];
        vec![
            K::Nil,
            K::Bool(true),
            K::Number(1.5),
            K::Number(-0.0),
            K::Number(f64::INFINITY),
            K::Number(f64::NEG_INFINITY),
            K::Number(9007199254740993.0),
            K::String(StringRef(0)),
            K::ReaderLiteral {
                kind: CompilerLiteralKind::Regex,
                items: vec![e],
                quoted: true,
            },
            K::DefinitionTest {
                value: e,
                unit: UnitRef(0),
            },
            K::WithMeta {
                value: e,
                metadata: e,
            },
            K::Var {
                definition: true,
                value: e,
                symbol: NamedRef(0),
                metadata: Some(VarMetadata::Global(GlobalRef::External(0))),
            },
            K::Var {
                definition: false,
                value: e,
                symbol: NamedRef(1),
                metadata: Some(VarMetadata::Expr(e)),
            },
            K::Keyword(NamedRef(0)),
            K::Symbol(NamedRef(1)),
            K::List(vec![e, e]),
            K::Vector(vec![e]),
            K::Map(vec![e, e]),
            K::Set(vec![]),
            K::LiteralMap(vec![e, e]),
            K::LiteralSet(vec![e]),
            K::CoreFn(CoreFnId::ALL[CoreFnId::ALL.len() - 1]),
            K::Local(l),
            K::Global(GlobalRef::Own(0)),
            K::Global(GlobalRef::External(0)),
            K::JsGlobal(JsGlobalRef(0)),
            K::ThisAs {
                binding: l,
                body: e,
            },
            K::JsArguments,
            K::JsTemplate {
                segments: vec!["(".into(), ")".into()],
                args: vec![e],
            },
            K::JsIndex {
                receiver: e,
                indices: vec![e],
            },
            K::JsDelete {
                receiver: e,
                key: e,
            },
            K::QualifiedMethod {
                class: e,
                method: Some("parse".into()),
            },
            K::CopyArguments(e),
            K::DefProtocol {
                global: GlobalRef::Own(0),
                methods: vec![protocol_method.clone()],
                via_metadata: true,
            },
            K::ProtocolPredicate {
                protocol: e,
                value: e,
                key: "app$core$P$".into(),
                direct: true,
            },
            K::ExtendType {
                instance: true,
                return_target: false,
                target: e,
                methods: methods.clone(),
            },
            K::Reify {
                metadata: Some(e),
                captures: vec![l],
                methods: methods.clone(),
            },
            K::DefType {
                record: true,
                global: GlobalRef::Own(0),
                methods,
                layout: Box::new(TypeLayout {
                    constructor: Some(GlobalRef::Own(0)),
                    body: Some(e),
                    masks: None,
                    map_constructor: Some(GlobalRef::External(0)),
                    fields: vec!["a".into(), "b".into()],
                }),
            },
            K::Construct {
                callee: e,
                args: vec![e],
            },
            K::Assert {
                test: e,
                message: Some(e),
                form: StringRef(0),
            },
            K::Def {
                global: GlobalRef::Own(0),
                init: Some(e),
                test: None,
                once: true,
            },
            K::SetGlobal {
                target: e,
                value: e,
            },
            K::Binding {
                bindings: vec![(e, e)],
                body: e,
            },
            K::WithOutStr(e),
            K::Exists(e),
            K::Loadable {
                target: LazyRef(0),
                class: e,
            },
            K::LoadableType,
            K::BindingExists(e),
            K::BooleanHint(e),
            K::Property {
                receiver: e,
                property: "length".into(),
            },
            K::MethodCall {
                receiver: e,
                method: "push".into(),
                args: vec![e, e],
            },
            K::If {
                test: e,
                yes: e,
                no: e,
            },
            K::Do(vec![e]),
            K::Let {
                bindings: vec![(l, e)],
                body: e,
            },
            K::LetFn {
                bindings: vec![(l, e)],
                body: e,
            },
            K::Loop {
                target: RecurRef(0),
                bindings: vec![(l, e)],
                body: e,
            },
            K::Recur {
                target: RecurRef(0),
                args: vec![e],
            },
            K::Throw(e),
            K::Await(e),
            K::Try {
                body: e,
                catches: vec![CatchClause {
                    class: Some(e),
                    binding: l,
                    body: e,
                }],
                finally: Some(e),
            },
            K::Case {
                strict: true,
                keyword_tests: false,
                test: e,
                branches: vec![(e, e)],
                default: Some(e),
            },
            K::Fn {
                name: Some(l),
                methods: vec![method],
            },
            K::Call {
                callee: e,
                args: vec![e],
            },
            K::InstanceOf { class: e, value: e },
        ]
    }

    fn reader_context() -> ReaderContext {
        ReaderContext {
            aliases: vec![("a".into(), "lib.a".into())],
            refers: vec![("x".into(), "lib.a".into(), "x".into())],
            excludes: vec!["map".into()],
            macros: vec!["m".into()],
        }
    }

    /// An artifact using every type, variant and optional field, with at
    /// least `exprs` expressions.
    fn sample(exprs: usize) -> NamespaceArtifact {
        let mut kinds = every_kind();
        while kinds.len() < exprs {
            kinds.push(ExprKind::Nil);
        }
        let contexts = [
            ExprContext::Value,
            ExprContext::Discard,
            ExprContext::Return,
        ];
        let count = kinds.len() as u32;
        let ir = SemanticIr {
            exprs: kinds
                .into_iter()
                .enumerate()
                .map(|(index, kind)| Expr {
                    kind,
                    span: (index as u32, index as u32 + 4),
                    context: contexts[index % 3],
                    source: if index % 5 == 0 {
                        SourceRef::Namespace(0)
                    } else {
                        SourceRef::Own
                    },
                })
                .collect(),
            locals: vec![
                LocalBinding {
                    name: "x".into(),
                    span: (1, 2),
                    source: SourceRef::Own,
                },
                LocalBinding {
                    name: "rest".into(),
                    span: (3, 7),
                    source: SourceRef::Namespace(0),
                },
            ],
            recur_targets: vec![RecurTarget {
                params: vec![LocalRef(0)],
                protocol_impl: true,
                receiver_from_this: false,
                used: true,
                argument_slots: vec![Some(0), None, Some(u32::MAX - 1)],
            }],
            strings: vec![units("text"), vec![0xdc00], units("app.core"), units("f")],
            named_literals: vec![
                NamedLiteral {
                    namespace: Some(StringRef(2)),
                    name: StringRef(3),
                },
                NamedLiteral {
                    namespace: None,
                    name: StringRef(0),
                },
            ],
            js_globals: vec![vec!["Math".into(), "max".into()]],
        };
        let code = RelocatableCode {
            text: "(function(){return globalThis.__lightCompilerTest(4);})".into(),
            units: vec![((50, 51), UnitRef(0))],
        };
        let view = RegistrationView {
            own_globals: 1,
            added: vec![GlobalRef::Own(0), GlobalRef::External(0)],
            reader_context: Some(reader_context()),
        };
        let site = Site {
            source: "app/core.cljs".into(),
            bytes: (10, 40),
        };
        NamespaceArtifact {
            header: Header {
                namespace: "app.core".into(),
                path: "/src/app/core.cljs".into(),
                source_length: 1234,
                cljc: true,
                source_name: Some("app/core.cljs".into()),
                entry: true,
                root: ExprRef(count - 1),
                policy: AnalysisPolicy {
                    warning_mode: true,
                    checked_arrays: false,
                    array_checks: ArrayChecks::Error,
                },
            },
            declarations: Declarations {
                globals: vec![Global {
                    namespace: "app.core".into(),
                    name: "f".into(),
                    flags: GlobalFlags {
                        type_descriptor: true,
                        declared: true,
                        constant: false,
                        define: true,
                        boolean_tag: true,
                    },
                    metadata: Some(ExprRef(0)),
                    span: (5, 9),
                    source: SourceRef::Own,
                    private: Some(true),
                    binding: Some(BindingKind::Macro),
                }],
                protocols: vec![(
                    GlobalRef::Own(0),
                    vec![ProtocolMethod {
                        global: GlobalRef::External(0),
                        key: "k".into(),
                        arities: vec![0],
                        variadic: None,
                    }],
                )],
                record_constructors: vec![GlobalRef::External(0)],
                field_bindings: vec![(LocalRef(1), RecurRef(0), true)],
                type_methods: vec![RecurRef(0)],
                macro_functions: vec![ExprRef(1)],
                macros: vec![MacroDeclaration {
                    name: "m".into(),
                    generation: 2,
                    metadata: every_datum(),
                    definition: site.clone(),
                    code: code.clone(),
                    view: view.clone(),
                }],
                aliases: vec![("a".into(), "lib.a".into())],
                requires: vec!["lib.a".into(), "lib.b".into()],
                refers: vec![("y".into(), "lib.b".into(), "z".into())],
                excludes: vec!["filter".into()],
                js_refers: vec![
                    ("max".into(), JsRefer::Global("Math.max".into())),
                    ("react".into(), JsRefer::Package("react".into(), None)),
                    (
                        "useState".into(),
                        JsRefer::Package("react".into(), Some("useState".into())),
                    ),
                ],
                reader_context: reader_context(),
                imports: vec![
                    Import {
                        namespace: "lib.a".into(),
                        alias: Some("a".into()),
                        refers: vec!["x".into()],
                    },
                    Import::default(),
                ],
                renames: vec![("m2".into(), "lib.a".into(), "m".into())],
                namespace_imports: vec![("Date".into(), "goog.date.Date".into())],
                namespace_symbol: Some(datum(DatumKind::Symbol(None, units("app.core")))),
                namespace_doc: None,
                lazy_targets: vec![LazyTarget {
                    site: ExprRef(0),
                    namespace: "lib.a".into(),
                    global: Some(GlobalRef::External(0)),
                }],
            },
            ir,
            patches: vec![GlobalPatch {
                global: GlobalRef::External(0),
                flags: GlobalFlags {
                    declared: true,
                    ..GlobalFlags::default()
                },
                metadata: None,
            }],
            units: DeferredUnits {
                units: vec![
                    Unit {
                        name: UnitName::Var("app.core/f".into()),
                        generation: 1,
                        analyses: 3,
                        emitted: true,
                        code: code.clone(),
                        view: view.clone(),
                        extension_frontier: 2,
                    },
                    Unit {
                        name: UnitName::Declaration,
                        generation: 1,
                        analyses: 0,
                        emitted: false,
                        code: RelocatableCode::default(),
                        view: RegistrationView::default(),
                        extension_frontier: 0,
                    },
                    Unit {
                        name: UnitName::Test("app.core/f".into()),
                        generation: 4,
                        analyses: 1,
                        emitted: false,
                        code,
                        view,
                        extension_frontier: 1,
                    },
                ],
                registrations: vec![
                    Registration::Root {
                        name: "app.core/f".into(),
                        unit: Some(UnitRef(0)),
                        test: Some(UnitRef(2)),
                        once: false,
                        location: "app/core.cljs:3:1".into(),
                    },
                    Registration::Root {
                        name: "app.core/g".into(),
                        unit: None,
                        test: None,
                        once: true,
                        location: "app/core.cljs:4:1".into(),
                    },
                    Registration::Bundle {
                        products: vec!["app.core/T".into()],
                        unit: UnitRef(1),
                        extensions: vec![
                            Extension {
                                protocol: "app.core/P".into(),
                                selector: Some("string".into()),
                                identity: ExtensionIdentity::Method(77),
                            },
                            Extension {
                                protocol: "cljs.core/IFn".into(),
                                selector: None,
                                identity: ExtensionIdentity::Text("app.core/T".into()),
                            },
                        ],
                        once: false,
                        group: true,
                        location: "app/core.cljs:5:1".into(),
                    },
                ],
            },
            diagnostics: Diagnostics {
                warnings: vec![Warning {
                    kind: "undeclared-var".into(),
                    message: "Use of undeclared Var app.core/q".into(),
                    span: (20, 21),
                    source: SourceRef::Own,
                }],
                expansions: vec![MacroExpansion {
                    macro_namespace: "lib.a".into(),
                    name: "m".into(),
                    generation: 1,
                    call: (30, 35),
                    source: SourceRef::Namespace(0),
                    definition: site,
                }],
            },
            references: References {
                namespaces: vec!["lib.a".into()],
                globals: vec![ExternalGlobal {
                    namespace: "lib.a".into(),
                    name: "x".into(),
                    declared: true,
                    private: Some(false),
                }],
            },
            names: GeneratedNames {
                from: Counters {
                    syntax_quote: 1,
                    anonymous: 2,
                    binding: 3,
                    destructure: 4,
                },
                to: Counters {
                    syntax_quote: 10,
                    anonymous: 20,
                    binding: 30,
                    destructure: 1 << 40,
                },
                gensym_to: 99,
                gensyms: vec![units("G__98"), units("x99")],
            },
            accounting: Some(Accounting {
                expansions: 7,
                generated_nodes: 300,
                node_peak: u64::MAX,
                macro_depth: 3,
                analysis_depth: 129,
                unit_depth: u32::MAX,
                conversion_peak: 41,
            }),
        }
    }

    fn fields() -> ManifestFields {
        ManifestFields {
            front_end_key: crate::cache_key::FrontEndKey {
                digest: [7; 32],
                components: std::array::from_fn(|at| [at as u8 + 10; 32]),
                dependencies: vec![crate::cache_key::DependencyKey {
                    namespace: "lib.a".into(),
                    kinds: 0b1011,
                    key: [8; 32],
                }],
            },
            emission_key: [9; 32],
            dependencies: vec![Dependency {
                namespace: "lib.a".into(),
                kinds: 0b1011,
            }],
            inputs: vec![
                ExternalInput::Resource(ResourceDependency {
                    path: "app/data.txt".into(),
                    candidates: vec![
                        Candidate {
                            path: "/a/app/data.txt".into(),
                            present: false,
                        },
                        Candidate {
                            path: "/b/app/data.txt".into(),
                            present: true,
                        },
                    ],
                    found: Some(ResolvedResource {
                        identity: Some("/b/app/data.txt".into()),
                        sha256: [0x5a; 32],
                        bytes: 300,
                    }),
                    stable: true,
                }),
                ExternalInput::Resource(ResourceDependency {
                    path: "app/missing.txt".into(),
                    candidates: Vec::new(),
                    found: None,
                    stable: false,
                }),
                ExternalInput::Resource(ResourceDependency {
                    path: "memory.txt".into(),
                    candidates: Vec::new(),
                    found: Some(ResolvedResource {
                        identity: None,
                        sha256: [0xff; 32],
                        bytes: 0,
                    }),
                    stable: false,
                }),
                ExternalInput::Namespace("lib.b".into()),
                ExternalInput::Provider {
                    token: ":host/schema".into(),
                    fingerprint: "v7".into(),
                },
                ExternalInput::Configuration { sha256: [0xc5; 32] },
                ExternalInput::View(BroadView::Namespaces),
                ExternalInput::View(BroadView::Declarations),
                ExternalInput::View(BroadView::Config),
            ],
            coarse: true,
            no_cache: false,
            always: true,
        }
    }

    fn decoded(bytes: &[u8]) -> Result<NamespaceArtifact, DecodeError> {
        decode(bytes, &DecodeLimits::default(), &|| false).map(|(_, artifact)| artifact)
    }

    /// The string table of encoded bytes.
    fn table(bytes: &[u8]) -> Vec<Vec<u16>> {
        let manifest = read_manifest(bytes, &DecodeLimits::default()).unwrap();
        let strings = manifest.sections[0];
        let mut input = Input {
            bytes: &bytes[strings.offset as usize..(strings.offset + strings.length) as usize],
            position: 0,
            part: "strings",
            limits: &DecodeLimits::default(),
        };
        input
            .string_table(&|| false, &Budget(std::cell::Cell::new(u64::MAX)))
            .unwrap()
    }

    fn table_section(entries: &[Vec<u16>]) -> Vec<u8> {
        let mut out = Vec::new();
        varint(&mut out, entries.len() as u64);
        for entry in entries {
            varint(&mut out, entry.len() as u64);
            for unit in entry {
                out.extend_from_slice(&unit.to_le_bytes());
            }
        }
        out
    }

    /// `bytes` with `section`'s content replaced by `content`, sealed.
    fn replace_section(bytes: &[u8], section: Section, content: &[u8]) -> Vec<u8> {
        let manifest = read_manifest(bytes, &DecodeLimits::default()).unwrap();
        let mut out = bytes[..manifest.sections[0].offset as usize].to_vec();
        for (index, bounds) in manifest.sections.iter().enumerate() {
            let start = bounds.offset as usize;
            let original = &bytes[start..start + bounds.length as usize];
            let new = if bounds.section == section {
                content
            } else {
                original
            };
            let entry = MANIFEST_START + 1 + index * SECTION_ENTRY;
            let offset = out.len() as u32;
            out[entry + 1..entry + 5].copy_from_slice(&offset.to_le_bytes());
            out[entry + 5..entry + 9].copy_from_slice(&(new.len() as u32).to_le_bytes());
            out.extend_from_slice(new);
        }
        seal(&mut out).unwrap();
        out
    }

    #[test]
    fn every_kind_round_trips() {
        let artifact = sample(0);
        artifact.validate().unwrap();
        let bytes = encode(&artifact, &fields()).unwrap();
        let (manifest, back) = decode(&bytes, &DecodeLimits::default(), &|| false).unwrap();
        assert_eq!(back, artifact);
        assert_eq!(encode(&back, &fields()).unwrap(), bytes);
        assert_eq!(manifest.fields, fields());
        assert_eq!(manifest.namespace, "app.core");
        assert_eq!(manifest.path, "/src/app/core.cljs");
        assert_eq!(manifest.source_length, 1234);
        assert_eq!(manifest.requires, ["lib.a", "lib.b"]);
        assert_eq!(manifest.references, ["lib.a"]);
        assert!(manifest.cljc && manifest.entry);
        assert_eq!(manifest.core_table, core_table());
        assert_eq!(manifest.length, bytes.len() as u64);
        // Interned: each text once.
        let strings = table(&bytes);
        let mut distinct = strings.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), strings.len());
        let mut artifact = artifact;
        artifact.accounting = None;
        artifact.header.source_name = None;
        artifact.header.cljc = false;
        assert_eq!(
            decoded(&encode(&artifact, &ManifestFields::default()).unwrap()).unwrap(),
            artifact
        );
    }

    #[test]
    fn special_numbers_keep_their_bits() {
        let values = [
            -0.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            f64::from_bits(0x7ff8_dead_beef_0001),
            f64::from_bits(0xfff0_0000_0000_0001),
            f64::MIN_POSITIVE / 2.0,
        ];
        let mut artifact = sample(0);
        let shift = values.len() as u32;
        // The numbers come first; every other reference moves past them.
        let mut exprs: Vec<Expr> = values
            .iter()
            .map(|&value| Expr {
                kind: ExprKind::Number(value),
                span: (0, 0),
                context: ExprContext::Value,
                source: SourceRef::Own,
            })
            .collect();
        exprs.append(&mut artifact.ir.exprs);
        artifact.ir.exprs = exprs;
        artifact.declarations.macros[0].metadata = datum(DatumKind::Collection {
            kind: CollectionKind::Vector,
            children: values.map(|v| datum(DatumKind::Double(v))).to_vec(),
        });
        artifact.header.root = ExprRef(artifact.header.root.0 + shift);
        let back = decoded(&encode(&artifact, &fields()).unwrap()).unwrap();
        for (index, value) in values.iter().enumerate() {
            let ExprKind::Number(decoded) = back.ir.exprs[index].kind else {
                panic!("expression {index} is not a number");
            };
            assert_eq!(decoded.to_bits(), value.to_bits());
        }
        let DatumKind::Collection { children, .. } = &back.declarations.macros[0].metadata.kind
        else {
            panic!("metadata is not a vector");
        };
        for (child, value) in children.iter().zip(values) {
            let DatumKind::Double(decoded) = child.kind else {
                panic!("not a double");
            };
            assert_eq!(decoded.to_bits(), value.to_bits());
        }
    }

    #[test]
    fn a_string_index_outside_the_table_is_refused() {
        let bytes = encode(&sample(0), &fields()).unwrap();
        // An empty table: every string reference is outside it.
        let error = decoded(&replace_section(&bytes, Section::Strings, &[0])).unwrap_err();
        assert!(
            matches!(error, DecodeError::Index { what: "string", .. }),
            "{error:?}"
        );
        // The full table but its last entry.
        let mut entries = table(&bytes);
        entries.pop();
        let error = decoded(&replace_section(
            &bytes,
            Section::Strings,
            &table_section(&entries),
        ))
        .unwrap_err();
        assert!(
            matches!(error, DecodeError::Index { what: "string", index, len } if index == len),
            "{error:?}"
        );
    }

    #[test]
    fn rust_text_must_be_valid_utf16() {
        // `ir.strings` keep their code units, but a local's name is a Rust
        // string: give the table entry it names a lone surrogate.
        let mut artifact = sample(0);
        artifact.ir.locals[0].name = "only-a-local-name".into();
        let bytes = encode(&artifact, &fields()).unwrap();
        let entries = table(&bytes)
            .into_iter()
            .map(|entry| {
                if entry == units("only-a-local-name") {
                    vec![0xdc00]
                } else {
                    entry
                }
            })
            .collect::<Vec<_>>();
        let corrupt = replace_section(&bytes, Section::Strings, &table_section(&entries));
        assert_eq!(decoded(&corrupt).unwrap_err(), DecodeError::Text("ir"));
    }

    #[test]
    fn over_deep_reader_data_is_refused() {
        let mut deep = datum(DatumKind::Nil);
        for _ in 0..DecodeLimits::default().max_depth + 1 {
            deep = datum(DatumKind::Collection {
                kind: CollectionKind::Vector,
                children: vec![deep],
            });
        }
        let mut artifact = sample(0);
        artifact.declarations.macros[0].metadata = deep;
        let error = encode(&artifact, &fields()).unwrap_err();
        assert!(error.contains("nests deeper"), "{error}");
        let bytes = encode(&sample(0), &fields()).unwrap();
        let limits = DecodeLimits {
            max_depth: 2,
            ..DecodeLimits::default()
        };
        assert_eq!(
            decode(&bytes, &limits, &|| false).unwrap_err(),
            DecodeError::TooDeep(2)
        );
    }

    #[test]
    fn sizes_and_counts_beyond_the_limits_are_refused() {
        let bytes = encode(&sample(0), &fields()).unwrap();
        let limited = |change: fn(&mut DecodeLimits)| {
            let mut limits = DecodeLimits::default();
            change(&mut limits);
            decode(&bytes, &limits, &|| false).unwrap_err()
        };
        assert!(matches!(
            limited(|l| l.max_bytes = 100),
            DecodeError::Limit { what: "bytes", .. }
        ));
        assert!(matches!(
            limited(|l| l.max_entries = 3),
            DecodeError::Limit {
                what: "entries",
                ..
            }
        ));
        assert!(matches!(
            limited(|l| l.max_string = 3),
            DecodeError::Limit {
                what: "string code units",
                ..
            }
        ));
        // A count beyond the limit, or beyond the bytes left, is refused
        // before anything is allocated for it.
        let mut huge = Vec::new();
        varint(&mut huge, 1 << 40);
        let corrupt = replace_section(&bytes, Section::Diagnostics, &huge);
        assert!(matches!(
            decoded(&corrupt),
            Err(DecodeError::Limit {
                what: "entries",
                ..
            })
        ));
        let mut many = Vec::new();
        varint(&mut many, 1000);
        let corrupt = replace_section(&bytes, Section::Diagnostics, &many);
        assert_eq!(
            decoded(&corrupt).unwrap_err(),
            DecodeError::Truncated("diagnostics")
        );
        let corrupt = replace_section(&bytes, Section::Diagnostics, &[0xff; 11]);
        assert_eq!(
            decoded(&corrupt).unwrap_err(),
            DecodeError::Overflow("diagnostics")
        );
        let manifest = read_manifest(&bytes, &DecodeLimits::default()).unwrap();
        let section = manifest.sections[4];
        let mut longer =
            bytes[section.offset as usize..(section.offset + section.length) as usize].to_vec();
        longer.push(0);
        assert_eq!(
            decoded(&replace_section(&bytes, Section::Diagnostics, &longer)).unwrap_err(),
            DecodeError::Trailing("diagnostics")
        );
    }

    #[test]
    fn long_tables_poll_for_cancellation() {
        let bytes = encode(&sample(5000), &fields()).unwrap();
        let polls = std::cell::Cell::new(0);
        decode(&bytes, &DecodeLimits::default(), &|| {
            polls.set(polls.get() + 1);
            false
        })
        .unwrap();
        assert!(polls.get() >= 4, "{} polls", polls.get());
        assert_eq!(
            decode(&bytes, &DecodeLimits::default(), &|| true).unwrap_err(),
            DecodeError::Cancelled
        );
    }

    #[test]
    fn the_manifest_reads_without_the_sections() {
        let mut bytes = encode(&sample(0), &fields()).unwrap();
        let manifest = read_manifest(&bytes, &DecodeLimits::default()).unwrap();
        let ir = manifest.sections[2];
        assert_eq!(ir.section, Section::Ir);
        bytes[ir.offset as usize + ir.length as usize / 2] ^= 0x10;
        assert_eq!(
            read_manifest(&bytes, &DecodeLimits::default()).unwrap(),
            manifest
        );
        assert_eq!(decoded(&bytes).unwrap_err(), DecodeError::Integrity("ir"));
        // A manifest from another core operation table.
        let mut other = encode(&sample(0), &fields()).unwrap();
        let at = other
            .windows(32)
            .position(|window| window == core_table())
            .unwrap();
        other[at] ^= 1;
        seal(&mut other).unwrap();
        assert_eq!(decoded(&other).unwrap_err(), DecodeError::CoreTable);
    }

    #[test]
    fn an_invalid_reference_fails_validation_after_decoding() {
        let mut artifact = sample(0);
        artifact.header.root = ExprRef(10_000);
        let bytes = encode(&artifact, &fields()).unwrap();
        assert!(matches!(decoded(&bytes), Err(DecodeError::Invalid(_))));
    }

    #[test]
    fn repeated_string_references_respect_the_decoded_budget() {
        // One string of 1,000 code units named 2,000 times: a few kilobytes
        // encoded, 4 MB decoded.
        let mut artifact = sample(0);
        let long = vec![0x61; 1000];
        artifact.names.gensyms = vec![long; 2000];
        let bytes = encode(&artifact, &fields()).unwrap();
        assert!(bytes.len() < 20_000, "{} bytes", bytes.len());
        let limits = DecodeLimits {
            max_decoded_bytes: 1 << 20,
            ..DecodeLimits::default()
        };
        assert!(matches!(
            decode(&bytes, &limits, &|| false).unwrap_err(),
            DecodeError::Limit {
                what: "decoded bytes",
                ..
            }
        ));
        decoded(&bytes).unwrap();
    }

    #[test]
    fn external_inputs_respect_the_decoded_budget() {
        let limits = DecodeLimits {
            max_decoded_bytes: 1 << 20,
            ..DecodeLimits::default()
        };
        let over = |inputs: Vec<ExternalInput>| {
            let bytes = encode(&sample(0), &ManifestFields { inputs, ..fields() }).unwrap();
            decoded(&bytes).unwrap();
            for error in [
                read_manifest(&bytes, &limits).unwrap_err(),
                decode(&bytes, &limits, &|| false).unwrap_err(),
            ] {
                assert!(
                    matches!(
                        error,
                        DecodeError::Limit {
                            what: "decoded bytes",
                            ..
                        }
                    ),
                    "{error:?}"
                );
            }
        };
        // A 2 MiB fingerprint.
        over(vec![ExternalInput::Provider {
            token: "p".into(),
            fingerprint: "f".repeat(2 << 20),
        }]);
        // 60,000 one-byte candidates, each a path of 24 bytes and more.
        over(vec![ExternalInput::Resource(ResourceDependency {
            path: "x".into(),
            candidates: (0..60_000)
                .map(|_| Candidate {
                    path: "".into(),
                    present: false,
                })
                .collect(),
            found: None,
            stable: true,
        })]);
    }

    #[test]
    fn cyclic_ir_is_refused_and_shared_ir_is_not() {
        let mut artifact = sample(0);
        let root = artifact.header.root;
        artifact.ir.exprs[root.0 as usize].kind = ExprKind::Do(vec![root]);
        let error = decoded(&encode(&artifact, &fields()).unwrap()).unwrap_err();
        assert!(
            matches!(&error, DecodeError::Invalid(reason) if reason.contains("contains itself")),
            "{error:?}"
        );
        // Two expressions that refer to each other.
        let mut artifact = sample(0);
        artifact.ir.exprs[0].kind = ExprKind::Throw(ExprRef(1));
        artifact.ir.exprs[1].kind = ExprKind::Throw(ExprRef(0));
        assert!(matches!(
            decoded(&encode(&artifact, &fields()).unwrap()),
            Err(DecodeError::Invalid(_))
        ));
        // `sample` shares expression 0 among nearly every other expression.
        let artifact = sample(0);
        assert_eq!(artifact.ir_depth(), Ok(2));
        assert_eq!(
            decoded(&encode(&artifact, &fields()).unwrap()).unwrap(),
            artifact
        );
    }

    #[test]
    fn expression_chains_deeper_than_the_limit_are_refused() {
        let mut artifact = sample(0);
        let start = artifact.ir.exprs.len() as u32;
        for index in 0..100 {
            artifact.ir.exprs.push(Expr {
                kind: ExprKind::Throw(ExprRef(if index == 0 { 0 } else { start + index - 1 })),
                span: (0, 0),
                context: ExprContext::Value,
                source: SourceRef::Own,
            });
        }
        artifact.header.root = ExprRef(start + 99);
        assert_eq!(artifact.ir_depth(), Ok(101));
        let bytes = encode(&artifact, &fields()).unwrap();
        decoded(&bytes).unwrap();
        let limits = DecodeLimits {
            max_ir_depth: 100,
            ..DecodeLimits::default()
        };
        assert_eq!(
            decode(&bytes, &limits, &|| false).unwrap_err(),
            DecodeError::Limit {
                what: "expression nesting",
                value: 101,
                limit: 100
            }
        );
    }

    #[test]
    fn large_strings_poll_for_cancellation() {
        // Few entries anywhere, but megabytes of text.
        let mut artifact = sample(0);
        artifact.ir.strings[0] = vec![0x62; 3 << 20];
        let bytes = encode(&artifact, &fields()).unwrap();
        let polls = std::cell::Cell::new(0);
        decode(&bytes, &DecodeLimits::default(), &|| {
            polls.set(polls.get() + 1);
            false
        })
        .unwrap();
        // The hashing of the 6 MB string table alone polls six times.
        assert!(polls.get() >= 8, "{} polls", polls.get());
        // Cancelled once decoding is under way.
        for after in [1, 3, 7] {
            let polls = std::cell::Cell::new(0);
            let error = decode(&bytes, &DecodeLimits::default(), &|| {
                polls.set(polls.get() + 1);
                polls.get() > after
            })
            .unwrap_err();
            assert_eq!(error, DecodeError::Cancelled, "after {after} polls");
        }
    }
}
