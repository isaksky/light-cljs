//! `light.lazy/loadable` in analysis (docs/code-splitting.md, "Phase 1"): the
//! special form, the loader that analyzes a lazy target's namespace after the
//! namespace naming it, the require graph the `Program` records, macro
//! expansions and the REPL. Until the `light.lazy` library exists, a loadable
//! evaluates to `nil`.
use super::optimization::execute;
use crate::phase_oracle::{Case, run};
use light_cljs::analyzer::{AnalysisOptions, LazyTargetId};
use light_cljs::compiler::analyze_namespace_entry_with_provider;
use light_cljs::eval_session::FileLoadRequest;
use light_cljs::source_provider::{LoadError, LoadRequest, SourceInput};
use light_cljs::{
    ApplicationOptions, EvalOptions, EvalSession, ExprKind, MacroHost, NamespaceCompileError,
    OptimizationLevel, Program, prepare_application,
};
use std::rc::Rc;

/// The in-memory source of each namespace by its path, as the browser
/// playground supplies sources.
fn provider(
    files: &'static [(&'static str, &'static str)],
) -> impl Fn(&LoadRequest) -> Result<Option<SourceInput>, LoadError> {
    move |request| {
        let namespace = String::from_utf16_lossy(request.namespace.name());
        let path = format!("{}.cljs", namespace.replace('.', "/").replace('-', "_"));
        Ok(files
            .iter()
            .find(|(file, _)| *file == path)
            .map(|(file, source)| SourceInput::new(*file, *source)))
    }
}

fn analyze(
    files: &'static [(&'static str, &'static str)],
) -> Result<Program, NamespaceCompileError> {
    analyze_namespace_entry_with_provider(
        "app.main",
        &provider(files),
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
}

/// The source text of expression `id`, from the file it was analyzed from.
fn text(files: &[(&str, &str)], program: &Program, id: light_cljs::ExprId) -> String {
    let expr = &program.exprs[id.0];
    let namespace = &program.namespaces[expr.source_index];
    let path = format!("{}.cljs", namespace.replace('.', "/"));
    let source = files.iter().find(|(file, _)| *file == path).unwrap().1;
    source[expr.span.clone()].to_owned()
}

/// Run the program at every level, call the function that is its value, and
/// return what the call prints, which must be the same at every level.
fn call_value_at_every_level(program: &Program) -> String {
    let outputs = OptimizationLevel::ALL.map(|level| {
        let js = prepare_application(program, ApplicationOptions::new(level)).emit_js();
        execute(&format!("const value = {js};\nconsole.log(value());"))
    });
    for (level, output) in OptimizationLevel::ALL.iter().zip(&outputs) {
        assert_eq!(output, &outputs[0], "at {level}");
    }
    outputs[0].clone()
}

static THREE: &[(&str, &str)] = &[
    (
        "app/main.cljs",
        "(ns app.main\n  (:require [app.shared :as shared]))\n\
         (swap! shared/log conj :main)\n\
         (def editor (light.lazy/loadable app.editor/start))\n\
         (def whole (light.lazy/loadable app.editor))\n\
         (defn ready [] :main-ready)\n\
         (fn [] (pr-str [(shared/label :main) @shared/log editor whole]))\n",
    ),
    (
        "app/shared.cljs",
        "(ns app.shared)\n(def log (atom []))\n(defn label [k] (str \"label \" (name k)))\n",
    ),
    // A lazy target may require the namespace that loads it.
    (
        "app/editor.cljs",
        "(ns app.editor\n  (:require [app.shared :as shared] [app.main :as main]))\n\
         (swap! shared/log conj (main/ready))\n\
         (defn start [] (shared/label :editor))\n",
    ),
];

#[test]
fn loadable_sites_and_the_require_graph_of_three_namespaces() {
    let program = analyze(THREE).unwrap_or_else(|error| panic!("{error}"));
    // app.editor is not a dependency of the entry: it is analyzed after it.
    assert_eq!(program.namespaces, ["app.shared", "app.main", "app.editor"]);
    assert_eq!(program.namespace_requires, [vec![], vec![0], vec![0, 1]]);
    assert_eq!(program.namespace_lazy_only, [false, false, true]);
    assert_eq!(program.lazy_namespaces, ["app.editor"]);
    let sites = program
        .lazy_targets
        .iter()
        .map(|target| {
            let global = target.global.map(|global| {
                let binding = &program.globals[global.0];
                assert!(binding.declared);
                format!("{}/{}", binding.namespace, binding.name)
            });
            (text(THREE, &program, target.site), target.namespace, global)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sites,
        [
            (
                "(light.lazy/loadable app.editor/start)".to_owned(),
                0,
                Some("app.editor/start".to_owned())
            ),
            ("(light.lazy/loadable app.editor)".to_owned(), 0, None),
        ]
    );
    for (index, target) in program.lazy_targets.iter().enumerate() {
        assert_eq!(
            program.exprs[target.site.0].kind,
            ExprKind::Loadable {
                target: LazyTargetId(index)
            }
        );
        assert_eq!(program.exprs[target.site.0].source_index, 1);
    }
    // The program's value is still app.main's last form, and app.editor's
    // top-level forms run after app.main's, which they read.
    assert_eq!(
        call_value_at_every_level(&program),
        "[\"label main\" [:main :main-ready] nil nil]\n"
    );
}

static DEPENDENCIES: &[(&str, &str)] = &[
    (
        "app/main.cljs",
        "(ns app.main (:require [app.util :as util]))\n\
         (def l (light.lazy/loadable app.editor/start))\n\
         (def m (light.lazy/loadable app.util/twice))\n\
         (fn [] (util/twice \"main\"))\n",
    ),
    (
        "app/util.cljs",
        "(ns app.util)\n(defn twice [s] (str s s))\n",
    ),
    (
        "app/editor.cljs",
        "(ns app.editor (:require [app.text :as text] [app.util :as util]))\n\
         (defn start [] (util/twice text/greeting))\n",
    ),
    ("app/text.cljs", "(ns app.text)\n(def greeting \"hi\")\n"),
];

/// A lazy target's unvisited dependencies are analyzed before it, after
/// everything already loaded; a target the entry requires is resident.
#[test]
fn a_lazy_target_follows_its_unvisited_dependencies_and_a_loaded_one_is_resident() {
    let program = analyze(DEPENDENCIES).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        program.namespaces,
        ["app.util", "app.main", "app.text", "app.editor"]
    );
    assert_eq!(
        program.namespace_requires,
        [vec![], vec![0], vec![], vec![2, 0]]
    );
    assert_eq!(program.namespace_lazy_only, [false, false, true, true]);
    assert_eq!(program.lazy_namespaces, ["app.editor", "app.util"]);
    assert_eq!(call_value_at_every_level(&program), "mainmain\n");

    static RESIDENT: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [app.util :as util]))\n\
             (def l (light.lazy/loadable app.util/twice))\n\
             (fn [] (util/twice \"main\"))\n",
        ),
        (
            "app/util.cljs",
            "(ns app.util)\n(defn twice [s] (str s s))\n",
        ),
    ];
    let program = analyze(RESIDENT).unwrap();
    assert_eq!(program.namespaces, ["app.util", "app.main"]);
    assert_eq!(program.namespace_lazy_only, [false, false]);
    let ExprKind::Do(roots) = &program.exprs[program.root.0].kind else {
        panic!("the root is a do")
    };
    assert_eq!(
        roots.len(),
        2,
        "no entry value is kept for a resident target"
    );
    assert_eq!(call_value_at_every_level(&program), "mainmain\n");
}

/// The entry's value survives namespaces analyzed after it, whatever its
/// last form is.
#[test]
fn the_entry_value_survives_lazy_namespaces_analyzed_after_it() {
    static VALUES: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main)\n\
             (do (def l (light.lazy/loadable app.lib)) (def n 2))\n\
             (do (def k 3) (fn [] (pr-str [l n k])))\n",
        ),
        ("app/lib.cljs", "(ns app.lib)\n(def x 1)\n"),
    ];
    let program = analyze(VALUES).unwrap();
    assert_eq!(program.namespace_lazy_only, [false, true]);
    assert_eq!(call_value_at_every_level(&program), "[nil 2 3]\n");
    // A definition last is the value it is when nothing follows the entry.
    static DEFINITION: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main)\n\
             (def l (light.lazy/loadable app.lib/f))\n\
             (defn start [] (str \"started \" (nil? l)))\n",
        ),
        ("app/lib.cljs", "(ns app.lib)\n(defn f [] 1)\n"),
    ];
    static RESIDENT_DEFINITION: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [app.lib]))\n\
             (def l (light.lazy/loadable app.lib/f))\n\
             (defn start [] (str \"started \" (nil? l)))\n",
        ),
        ("app/lib.cljs", "(ns app.lib)\n(defn f [] 1)\n"),
    ];
    let lazy = analyze(DEFINITION).unwrap();
    assert_eq!(lazy.namespace_lazy_only, [false, true]);
    let resident = analyze(RESIDENT_DEFINITION).unwrap();
    assert_eq!(resident.namespace_lazy_only, [false, false]);
    assert_eq!(call_value_at_every_level(&lazy), "started true\n");
    assert_eq!(call_value_at_every_level(&resident), "started true\n");
    // An entry without forms is nil; here a dependency of the entry holds
    // the lazy reference.
    static EMPTY: &[(&str, &str)] = &[
        ("app/main.cljs", "(ns app.main (:require [app.dep]))\n"),
        (
            "app/dep.cljs",
            "(ns app.dep)\n(def l (light.lazy/loadable app.lib))\n",
        ),
        ("app/lib.cljs", "(ns app.lib)\n(def x 1)\n"),
    ];
    let program = analyze(EMPTY).unwrap();
    assert_eq!(program.namespaces, ["app.dep", "app.main", "app.lib"]);
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(execute(&format!("console.log({js});")), "null\n", "{level}");
    }
}

fn error(files: &'static [(&'static str, &'static str)]) -> String {
    let error = analyze(files).expect_err("analysis fails");
    let path = error.path.as_ref().expect("a located error");
    let (line, column) = error.line_column.expect("a line and column");
    format!("{}:{line}:{column}: {}", path.display(), error.message)
}

#[test]
fn analysis_errors_name_the_site() {
    const LIB: (&str, &str) = ("app/lib.cljs", "(ns app.lib)\n(defn f [] 1)\n");
    macro_rules! case {
        ($main:expr) => {{
            static FILES: &[(&str, &str)] = &[("app/main.cljs", $main), LIB];
            error(FILES)
        }};
    }
    // A Var its namespace never defines, reported once every namespace is
    // analyzed.
    assert_eq!(
        case!("(ns app.main)\n(def x 1)\n  (light.lazy/loadable app.lib/missing)\n"),
        "app/main.cljs:3:3: lazy target app.lib/missing is not defined"
    );
    // A namespace no source provides.
    assert_eq!(
        case!("(ns app.main)\n(light.lazy/loadable app.nope/start)\n"),
        "app/main.cljs:2:1: lazy target namespace app.nope not found: expected app/nope.cljs or .cljc in explicit source roots"
    );
    assert_eq!(
        case!("(ns app.main)\n[(light.lazy/loadable app.nope)]\n"),
        "app/main.cljs:2:2: lazy target namespace app.nope not found: expected app/nope.cljs or .cljc in explicit source roots"
    );
    // Not a literal symbol.
    let literal = "light.lazy/loadable requires one literal symbol naming a Var or a namespace";
    assert_eq!(
        case!("(ns app.main)\n(light.lazy/loadable (symbol \"app.lib\"))\n"),
        format!("app/main.cljs:2:22: {literal}")
    );
    assert_eq!(
        case!("(ns app.main)\n(light.lazy/loadable 'app.lib/f)\n"),
        format!("app/main.cljs:2:22: {literal}")
    );
    // The symbol is the target as written: an unqualified one is a namespace.
    assert_eq!(
        case!("(ns app.main)\n(def s 'app.lib)\n(light.lazy/loadable s)\n"),
        "app/main.cljs:3:1: lazy target namespace s not found: expected s.cljs or .cljc in explicit source roots"
    );
    assert_eq!(
        case!("(ns app.main)\n(light.lazy/loadable)\n"),
        format!("app/main.cljs:2:1: {literal}")
    );
    assert_eq!(
        case!("(ns app.main)\n(light.lazy/loadable app.lib/f app.lib)\n"),
        format!("app/main.cljs:2:1: {literal}")
    );
    // A target in the namespace of the site.
    assert_eq!(
        case!("(ns app.main)\n(defn f [] 1)\n(light.lazy/loadable app.main/f)\n"),
        "app/main.cljs:3:22: lazy target app.main/f is in the current namespace app.main"
    );
    assert_eq!(
        case!("(ns app.main)\n(light.lazy/loadable app.main)\n"),
        "app/main.cljs:2:22: lazy target app.main is in the current namespace app.main"
    );
    // The core library is not a namespace a chunk can hold.
    assert_eq!(
        case!("(ns app.main)\n(light.lazy/loadable cljs.core/inc)\n"),
        "app/main.cljs:2:22: lazy target namespace cljs.core is not a source namespace"
    );
}

/// `analyze_namespaces` analyzes the sources it is given, in order: a lazy
/// target among them is resident, and one missing from them is an error.
#[test]
fn analyzing_given_namespaces_requires_lazy_targets_among_them() {
    use light_cljs::{NamespaceInput, analyze_namespaces, parse};
    let sources = [
        "(ns app.lib) (defn f [] 1)",
        "(ns app.main) (light.lazy/loadable app.lib/f) (light.lazy/loadable app.nope)",
    ];
    let syntax = sources.map(|source| parse(source).unwrap());
    let input = |index: usize, name| NamespaceInput {
        name,
        source: sources[index],
        syntax: &syntax[index],
        aliases: &[],
        requires: &[],
    };
    let (source, error) = analyze_namespaces(&[input(1, "app.main")]).unwrap_err();
    assert_eq!(
        (source, error.to_string(), &sources[1][error.span]),
        (
            0,
            "lazy target namespace app.lib not found".into(),
            "(light.lazy/loadable app.lib/f)"
        )
    );
    let (_, error) = analyze_namespaces(&[input(0, "app.lib"), input(1, "app.main")]).unwrap_err();
    assert_eq!(
        error.to_string(),
        "lazy target namespace app.nope not found"
    );
}

/// The special form is recognized through an alias of `light.lazy`, and only
/// through one: `loadable` of another namespace is an ordinary Var.
#[test]
fn an_alias_of_light_lazy_names_the_special_form() {
    static ALIASED: &[(&str, &str)] = &[
        (
            "app/main.cljs",
            "(ns app.main (:require [light.lazy :as lazy] [app.other :as other]))\n\
             (def l (lazy/loadable app.lib/f))\n\
             (fn [] (pr-str [l (other/loadable 'app.lib/f)]))\n",
        ),
        // A stand-in until the `light.lazy` library exists.
        ("light/lazy.cljs", "(ns light.lazy)\n"),
        (
            "app/other.cljs",
            "(ns app.other)\n(defn loadable [x] (str \"other \" x))\n",
        ),
        ("app/lib.cljs", "(ns app.lib)\n(defn f [] 1)\n"),
    ];
    let program = analyze(ALIASED).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(program.lazy_targets.len(), 1);
    assert_eq!(
        text(ALIASED, &program, program.lazy_targets[0].site),
        "(lazy/loadable app.lib/f)"
    );
    assert_eq!(
        program.namespaces,
        ["light.lazy", "app.other", "app.main", "app.lib"]
    );
    assert_eq!(
        call_value_at_every_level(&program),
        "[nil \"other app.lib/f\"]\n"
    );
}

static MACRO: &[(&str, &str)] = &[
    (
        "app/main.cljs",
        "(ns app.main (:require [light.lazy :as lazy]))\n\
         (defmacro later [] `(lazy/loadable app.lib/f))\n\
         (def l (later))\n\
         (fn [] (pr-str l))\n",
    ),
    ("light/lazy.cljs", "(ns light.lazy)\n"),
    ("app/lib.cljs", "(ns app.lib)\n(defn f [] 1)\n"),
];

/// A loadable a macro writes, here through syntax quote of an alias, is a
/// site at the macro's call.
#[test]
fn a_macro_expansion_can_write_a_loadable() {
    let program = analyze(MACRO).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(program.lazy_targets.len(), 1);
    let target = program.lazy_targets[0];
    assert_eq!(text(MACRO, &program, target.site), "(later)");
    let global = &program.globals[target.global.unwrap().0];
    assert_eq!(
        (global.namespace.as_str(), global.name.as_str()),
        ("app.lib", "f")
    );
    assert_eq!(program.namespace_lazy_only, [false, false, true]);
    assert_eq!(call_value_at_every_level(&program), "nil\n");
}

mod phase_oracles {
    use super::*;

    /// The macro host compiles the same loadable when a macro calls the
    /// function holding it: both phases analyze the form, and its target's
    /// namespace is analyzed only after the entry's.
    #[test]
    fn loadable_in_macro_generated_code() {
        run(&[Case::namespace(
            "macro-generated-loadable",
            "app.main",
            "(probe)",
            "(ns app.main)\n\
             (defmacro later [] `(light.lazy/loadable app.lib/f))\n\
             (defn probe [] (let [l (later)] (= l l)))\n",
        )
        .with_sources(&[("app/lib.cljs", "(ns app.lib)\n(defn f [] 1)\n")])]);
    }
}

/// A REPL session over `files`, evaluating source text to its printed value
/// or its error message after the name of the file the error is in.
fn repl(
    files: &'static [(&'static str, &'static str)],
) -> impl FnMut(&str) -> Result<String, String> {
    let mut session = EvalSession::new(EvalOptions {
        source_provider: Some(Rc::new(provider(files))),
        file_provider: Some(Rc::new(move |request: &FileLoadRequest| {
            Ok(files
                .iter()
                .find(|(file, _)| *file == request.path)
                .map(|(file, source)| SourceInput::new(*file, *source)))
        })),
        ..Default::default()
    })
    .unwrap();
    move |source| {
        session
            .eval_string(source)
            .map(|result| {
                let text = session.print(result.value).unwrap();
                session.release(result.value).unwrap();
                text
            })
            .map_err(|error| {
                let file = error.location.map(|span| span.source().name().to_owned());
                format!("{}: {}", file.unwrap_or_default(), error.message)
            })
    }
}

/// `app.editor` requires `app.main`, which holds a loadable of `app.editor`.
const EDITOR: (&str, &str) = (
    "app/editor.cljs",
    "(ns app.editor (:require [app.main :as main]))\n\
     (def loads (swap! main/loads inc))\n\
     (defn start [] [:started (main/ready)])\n",
);
const MAIN: (&str, &str) = (
    "app/main.cljs",
    "(ns app.main)\n\
     (def loads (atom 0))\n\
     (def editor (light.lazy/loadable app.editor/start))\n\
     (defn ready [] :ready)\n",
);

/// The REPL loads the namespace of a loadable it has not loaded, as a
/// `require` of it would: before a top-level form runs, and after the
/// namespace holding the loadable when that namespace is loading, as the
/// compiler's loader does, so a target may require the namespace naming it.
#[test]
fn the_repl_loads_the_namespace_a_loadable_names() {
    static BACK_EDGE: &[(&str, &str)] = &[EDITOR, MAIN];
    // The target first: app.main loads while app.editor does.
    let mut eval = repl(BACK_EDGE);
    assert_eq!(
        eval("(light.lazy/loadable app.editor/start)"),
        Ok("nil".into())
    );
    assert_eq!(
        eval("[(app.editor/start) app.main/editor @app.main/loads]"),
        Ok("[[:started :ready] nil 1]".into())
    );
    assert_eq!(
        eval("(light.lazy/loadable app.editor/stop)"),
        Err("<eval>.cljs: lazy target app.editor/stop is not defined".into())
    );
    assert_eq!(
        eval("(light.lazy/loadable app.nope)"),
        Err("<eval>.cljs: lazy target namespace app.nope not found".into())
    );
    assert_eq!(eval("(light.lazy/loadable app.editor)"), Ok("nil".into()));
    // The namespace naming the target first: app.editor loads after app.main.
    let mut eval = repl(BACK_EDGE);
    assert_eq!(eval("(require 'app.main)"), Ok("nil".into()));
    assert_eq!(
        eval("[(app.editor/start) @app.main/loads]"),
        Ok("[[:started :ready] 1]".into())
    );
}

/// A Var target is checked once every namespace the REPL loads for a form is
/// loaded, at the site that names it.
#[test]
fn the_repl_checks_lazy_targets_after_their_namespaces_load() {
    static MUTUAL: &[(&str, &str)] = &[
        (
            "app/a.cljs",
            "(ns app.a)\n(def b (light.lazy/loadable app.b))\n",
        ),
        (
            "app/b.cljs",
            "(ns app.b)\n(def a (light.lazy/loadable app.a/missing))\n",
        ),
    ];
    let mut eval = repl(MUTUAL);
    assert_eq!(
        eval("(light.lazy/loadable app.a)"),
        Err("app/b.cljs: lazy target app.a/missing is not defined".into())
    );
    // Nothing the failed form recorded carries over.
    assert_eq!(eval(":next"), Ok(":next".into()));
}

/// `load-file` evaluates its forms through a runtime callback, which loads
/// lazy targets the same way.
#[test]
fn load_file_loads_the_namespaces_its_loadables_name() {
    static FILES: &[(&str, &str)] = &[
        (
            "script.cljs",
            "(def l (light.lazy/loadable app.editor/start))\n[l (app.editor/start)]\n",
        ),
        ("undefined.cljs", "(light.lazy/loadable app.editor/stop)\n"),
        ("missing.cljs", "(light.lazy/loadable app.nope)\n"),
        ("back.cljs", "(require 'app.main)\n@app.main/loads\n"),
        EDITOR,
        MAIN,
    ];
    let mut eval = repl(FILES);
    assert_eq!(
        eval("(load-file \"script.cljs\")"),
        Ok("[nil [:started :ready]]".into())
    );
    assert_eq!(eval("@app.main/loads"), Ok("1".into()));
    let error = eval("(load-file \"undefined.cljs\")").unwrap_err();
    assert!(
        error.contains("lazy target app.editor/stop is not defined"),
        "{error}"
    );
    let error = eval("(load-file \"missing.cljs\")").unwrap_err();
    assert!(
        error.contains("lazy target namespace app.nope not found"),
        "{error}"
    );
    // The namespace naming the target first, through the callback.
    let mut eval = repl(FILES);
    assert_eq!(eval("(load-file \"back.cljs\")"), Ok("1".into()));
}
