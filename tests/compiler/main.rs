//! Compiler integration tests: one module per feature suite, linked into a single
//! test binary (`--test compiler`) so they build once and run in parallel. Add a
//! suite as `tests/compiler/<feature>.rs` and register it below with `mod <feature>;`.
//! Run one suite with `cargo nextest run --test compiler <feature>::` or
//! `cargo test --test compiler <feature>::`.

/// Declares one `#[test]` per fixture row or row range, so long fixture loops run in
/// parallel and a failure names its rows: `row_tests!(run; row_00(0), row_01(1));`
/// expands to `#[test] fn row_00() { run(0) }` and so on.
macro_rules! row_tests {
    ($run:ident; $($name:ident($($arg:expr),*)),+ $(,)?) => {
        $(
            #[test]
            fn $name() {
                $run($($arg),*);
            }
        )+
    };
}

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

mod array_settings;
mod atom_object_equiv;
mod bootstrap;
mod boundary_completion;
mod case_char_string_accepted;
mod case_char_string_gap;
mod case_numeric;
mod cljc_prefixes;
mod configured_custom_source_context;
mod configured_source_context;
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
mod destructuring_completion;
mod edn;
mod edn_stream;
mod eduction_contracts;
mod eductions;
mod equality;
mod equality_contracts;
mod extensions;
mod find_effects;
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
mod language_namespace;
mod library_bodies;
mod literal_char_string;
mod literal_map_traversal;
mod literal_numeric;
mod literal_set_traversal;
mod local_api_source_context;
mod macros;
mod map_entries;
mod map_reader_order;
mod map_value_equiv;
mod mapv_apply;
mod multimethod_evidence;
mod multimethods;
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
mod printing_completion;
mod protocols;
mod public_constructors;
mod public_print_helper_gap;
mod public_print_helpers;
mod public_protocols;
mod quoted_when;
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
mod source_metadata;
mod state;
mod string_fixtures;
mod string_join;
mod strings;
mod syntax_context;
mod test_metadata;
mod transducer_contracts;
mod transducers;
mod transients;
mod types;
mod utilities_bodies;
mod utilities_completion;
mod var_test_metadata;
mod warnings;
mod watch_keys;
