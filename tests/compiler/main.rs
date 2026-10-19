//! Compiler integration tests: one module per feature suite, linked into a single
//! test binary (`--test compiler`) so they build once and run in parallel. Add a
//! suite as `tests/compiler/<feature>.rs` and register it below with `mod <feature>;`.
//! Run one suite with `cargo nextest run --test compiler <feature>::` or
//! `cargo test --test compiler <feature>::`.

// `row_tests!` declares one test per fixture row, row range or share of the rows.
#[macro_use]
#[path = "../support/row_tests.rs"]
mod row_tests;

// The SCI wire protocol is shared by several suites; load it once.
#[path = "../support/sci_protocol.rs"]
#[allow(dead_code)]
mod sci_protocol;

// Complete sources in both compiler phases and host lifecycles, for the
// `phase_oracles` modules of several suites.
#[path = "../support/phase_oracle.rs"]
mod phase_oracle;

// Seeded program generators, shared with the execution binary.
#[path = "../support/generated.rs"]
#[allow(dead_code)]
mod generated;

// The Node driver for single-file `--esm` and chunked output.
#[path = "../support/chunked_execution.rs"]
mod chunked_execution;

// The Node driver for dev builds and their `none` oracles.
#[path = "../support/dev_execution.rs"]
mod dev_execution;

// Reload semantics as rows, shared with tests/dev_server.rs.
#[path = "../support/reload_fixtures.rs"]
mod reload_fixtures;

// Temporary directories removed when their test ends, however it ends.
#[path = "../support/temp_dir.rs"]
#[allow(dead_code)]
mod temp_dir;

// Just enough JSON for contract rows, driver records and chunks.json.
#[path = "../support/json.rs"]
#[allow(dead_code)]
mod json;

mod array_settings;
mod artifact_codec;
mod async_await;
mod atom_object_equiv;
mod bootstrap;
mod boundary_completion;
mod build_config;
mod build_resources;
mod builtin_types;
mod cache_contract;
mod cache_keys;
mod cache_modes;
mod cache_policy;
mod call_shapes;
mod case_char_string_accepted;
mod case_char_string_gap;
mod case_numeric;
mod chunks;
mod cljc_prefixes;
mod compile_time_units;
mod configured_custom_source_context;
mod configured_source_context;
mod constant_hashes;
mod constant_lookups;
mod constant_pool;
mod constant_table;
mod control;
mod core_extension_demand;
mod core_factories;
mod core_gate;
mod core_helpers;
mod core_operations;
mod core_var_metadata;
mod data_libraries;
mod declare_values;
mod deferred;
mod definitions;
mod dependency_manifest;
mod destructuring_completion;
mod dev_build;
mod edn;
mod edn_stream;
mod eduction_contracts;
mod eductions;
mod equality;
mod equality_contracts;
mod extensions;
mod features_example;
mod find_effects;
mod folding_inlining;
mod generated_names;
mod generated_optimization;
mod hash_branch_effects;
mod hash_map_from_arrays;
mod hash_map_source;
mod host_hooks;
mod hosted_import_source_context;
mod hosted_namespace_source_context;
mod ifn_public_context;
mod ifn_rest;
mod integer_predicates;
mod interop;
mod invoke_program_scope;
mod iterator_helpers;
mod iterator_name;
mod key_shapes;
mod language_namespace;
mod lazy;
mod library_bodies;
mod literal_char_string;
mod literal_map_traversal;
mod literal_numeric;
mod literal_set_traversal;
mod local_api_source_context;
mod macro_free_units;
mod macro_scalar_spans;
mod macros;
mod map_entries;
mod map_reader_order;
mod map_value_equiv;
mod mapv_apply;
mod multimethod_evidence;
mod multimethod_tables;
mod multimethods;
mod namespace_artifacts;
mod namespace_completion;
mod namespace_shadowing;
mod npm_packages;
mod ns_name;
mod object_equiv;
mod optimization;
mod original_eval_source_probe;
mod positional_constructors;
mod predicate_macro;
mod prim_seq;
mod primitives;
mod print_install;
mod printing_completion;
mod protocols;
mod public_constructors;
mod public_print_helper_gap;
mod public_print_helpers;
mod public_protocols;
mod quoted_when;
mod rapid_types;
mod reader;
mod record_map_constructor_metadata;
mod record_receiver_binding;
mod records;
mod reduce_kv_entries;
mod reflection_completion;
mod regex;
mod reify;
mod residual_bodies;
mod residual_completion;
mod residual_source_context;
mod runtime_manifest;
mod runtime_map_order;
mod runtime_map_traversal;
mod runtime_references;
mod runtime_set_order;
mod runtime_set_traversal;
mod runtime_slicing;
mod sci_cljc_accepted_probe;
mod sci_cljc_source_probe;
mod sci_execution;
mod sci_gap_phase_probe;
mod sci_library_context;
mod sci_library_reconciliation;
mod sci_record_declaration_context;
mod sci_record_duplicate_method_probe;
mod sci_record_method_closure_controls;
mod sci_record_method_dialect;
mod sci_record_source_context;
mod sci_service_probes;
mod sci_source_context;
mod sci_symbol_keyword_context;
mod sci_unresolved_source_probe;
mod sequence_bodies;
mod sequence_completion;
mod sequence_contracts;
mod sequence_object_equiv;
mod sequence_transducers;
mod sequences;
mod set_project;
mod size_ratchet;
mod sorted_collections;
mod sorted_contracts;
mod source_loading;
mod source_metadata;
mod state;
mod string_fixtures;
mod string_join;
mod strings;
mod syntax_context;
mod temp_dirs;
mod test_metadata;
mod transducer_contracts;
mod transducers;
mod transients;
mod type_inference;
mod types;
mod utilities_bodies;
mod utilities_completion;
mod utility_macros;
mod var_test_metadata;
mod warnings;
mod watch_keys;
mod worker_isolates;
