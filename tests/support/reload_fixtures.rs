//! Reload semantics as fixtures (docs/dev-workflow.md, "Verification"):
//! each row is a small program, an edit saved to one of its files, the
//! namespaces that edit reloads, and what the program prints, first when the
//! host calls `start`, then when the reload's `^:dev/after-load` hook shows
//! the state again. The rows run through the Phase 0 driver, which applies
//! the units of a fresh dev build of the edited sources to the running build
//! of the first ones (tests/compiler/dev_build.rs), and through `--watch
//! --serve`, whose client loads the step the edit makes and whose rewritten
//! units the driver applies too (tests/dev_server.rs): all paths print the
//! same.

/// One reload fixture.
pub struct ReloadFixture {
    pub name: &'static str,
    /// The source files, by path under the source root; the entry is
    /// `app.main`.
    pub files: &'static [(&'static str, &'static str)],
    /// The file the edit saves, and its new text.
    pub edit: (&'static str, &'static str),
    /// The namespaces the edit reloads, in load order.
    pub reloads: &'static [&'static str],
    /// What the program prints, started and then reloaded.
    pub stdout: &'static str,
}

/// The driver's record of a run that printed `stdout`, traced nothing and
/// returned undefined, as `start` does.
pub fn record(stdout: &str) -> String {
    let mut json = String::from("\"");
    for ch in stdout.chars() {
        match ch {
            '\n' => json.push_str("\\n"),
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            ch => json.push(ch),
        }
    }
    json.push('"');
    format!(
        r#"{{"id":"program","outcome":{{"status":"returned","value":{{"$value":"undefined"}}}},"trace":[],"stdout":{json}}}"#
    )
}

/// The entry of most rows: `start` shows `app.core`'s state, and so does
/// the after-load hook.
const MAIN: &str = r#"(ns app.main
  (:require [app.core :as core]))

(defn ^:dev/after-load reloaded [] (core/show))

(defn start [] (core/show))

start
"#;

pub const FIXTURES: &[ReloadFixture] = &[
    // A caller sees the new definition at its next call; a function saved
    // in a `defonce` keeps its old code.
    ReloadFixture {
        name: "functions",
        files: &[
            ("app/main.cljs", MAIN),
            (
                "app/core.cljs",
                r#"(ns app.core)

(defn greeting [] "hello")

(defonce saved greeting)

(defn show [] (js/console.log (greeting) (saved)))
"#,
            ),
        ],
        edit: (
            "app/core.cljs",
            r#"(ns app.core)

(defn greeting [] "goodbye")

(defonce saved greeting)

(defn show [] (js/console.log (greeting) (saved)))
"#,
        ),
        reloads: &["app.core"],
        stdout: "hello hello\ngoodbye hello\n",
    },
    // `defonce` keeps its value, nil included; `def` is initialized again.
    ReloadFixture {
        name: "defonce",
        files: &[
            ("app/main.cljs", MAIN),
            (
                "app/core.cljs",
                r#"(ns app.core)

(defonce kept (atom 1))

(def plain (atom 1))

(defonce absent nil)

(defn show []
  (swap! kept inc)
  (swap! plain inc)
  (js/console.log @kept @plain (pr-str absent)))
"#,
            ),
        ],
        edit: (
            "app/core.cljs",
            r#"(ns app.core)

(defonce kept (atom 100))

(def plain (atom 100))

(defonce absent :present)

(defn show []
  (swap! kept inc)
  (swap! plain inc)
  (js/console.log @kept @plain (pr-str absent)))
"#,
        ),
        reloads: &["app.core"],
        stdout: "2 2 nil\n3 101 nil\n",
    },
    // A redefined record has a new constructor: an instance made before
    // keeps its methods and is not an instance of the new type.
    ReloadFixture {
        name: "records",
        files: &[
            ("app/main.cljs", MAIN),
            (
                "app/core.cljs",
                r#"(ns app.core)

(defprotocol Area (area [shape]))

(defrecord Rect [w h] Area (area [_] (* w h)))

(defonce saved (->Rect 2 3))

(defn show []
  (js/console.log (area saved) (area (->Rect 2 3)) (instance? Rect saved) (pr-str saved)))
"#,
            ),
        ],
        edit: (
            "app/core.cljs",
            r#"(ns app.core)

(defprotocol Area (area [shape]))

(defrecord Rect [w h] Area (area [_] (+ w h)))

(defonce saved (->Rect 2 3))

(defn show []
  (js/console.log (area saved) (area (->Rect 2 3)) (instance? Rect saved) (pr-str saved)))
"#,
        ),
        reloads: &["app.core"],
        stdout: "6 6 true #app.core.Rect{:w 2, :h 3}\n6 5 false #app.core.Rect{:w 2, :h 3}\n",
    },
    // A redefined protocol, with a method more, still dispatches to the
    // extension a namespace that was not reloaded made.
    ReloadFixture {
        name: "protocols",
        files: &[
            (
                "app/main.cljs",
                r#"(ns app.main
  (:require [app.core :as core]
            [app.dogs]))

(defn ^:dev/after-load reloaded [] (core/show))

(defn start [] (core/show))

start
"#,
            ),
            (
                "app/core.cljs",
                r#"(ns app.core)

(defprotocol Speak (speak [animal]))

(defonce animals (atom []))

(defn show [] (js/console.log (pr-str (map speak @animals))))
"#,
            ),
            (
                "app/dogs.cljs",
                r#"(ns app.dogs
  (:require [app.core :as core]))

(deftype Dog [] core/Speak (speak [_] "woof"))

(swap! core/animals conj (Dog.))
"#,
            ),
        ],
        edit: (
            "app/core.cljs",
            r#"(ns app.core)

(defprotocol Speak (speak [animal]) (shout [animal]))

(defonce animals (atom []))

(defn show [] (js/console.log "speaks" (pr-str (map speak @animals))))
"#,
        ),
        reloads: &["app.core"],
        stdout: "(\"woof\")\nspeaks (\"woof\")\n",
    },
    // `defmulti` keeps its value, dispatch function and all; `defmethod`
    // replaces a method of it.
    ReloadFixture {
        name: "multimethods",
        files: &[
            ("app/main.cljs", MAIN),
            (
                "app/core.cljs",
                r#"(ns app.core)

(defmulti kind (fn [x] (if (string? x) :text :other)))

(defmethod kind :text [_] "text")

(defmethod kind :other [_] "other")

(defn show [] (js/console.log (kind "a") (kind 1)))
"#,
            ),
        ],
        edit: (
            "app/core.cljs",
            r#"(ns app.core)

(defmulti kind (fn [_] :other))

(defmethod kind :text [_] "TEXT")

(defmethod kind :other [_] "OTHER")

(defn show [] (js/console.log (kind "a") (kind 1)))
"#,
        ),
        reloads: &["app.core"],
        stdout: "text other\nTEXT OTHER\n",
    },
    // A changed macro reloads the namespace that requires it, recompiled
    // with the new expansion.
    ReloadFixture {
        name: "macros",
        files: &[
            ("app/main.cljs", MAIN),
            (
                "app/macros.cljs",
                r#"(ns app.macros)

(defmacro shout [text] `(str ~text "!"))
"#,
            ),
            (
                "app/core.cljs",
                r#"(ns app.core
  (:require [app.macros :refer [shout]]))

(defn show [] (js/console.log (shout "hey")))
"#,
            ),
        ],
        edit: (
            "app/macros.cljs",
            r#"(ns app.macros)

(defmacro shout [text] `(str ~text "!!!"))
"#,
        ),
        reloads: &["app.macros", "app.core"],
        stdout: "hey!\nhey!!!\n",
    },
    // A new require loads the namespace, ahead of its requirer.
    ReloadFixture {
        name: "ns-new-require",
        files: &[
            ("app/main.cljs", MAIN),
            (
                "app/core.cljs",
                r#"(ns app.core)

(defn show [] (js/console.log "alone"))
"#,
            ),
            (
                "app/extra.cljs",
                r#"(ns app.extra)

(defn hello [] "from extra")
"#,
            ),
        ],
        edit: (
            "app/core.cljs",
            r#"(ns app.core
  (:require [app.extra :as extra]))

(defn show [] (js/console.log (extra/hello)))
"#,
        ),
        reloads: &["app.extra", "app.core"],
        stdout: "alone\nfrom extra\n",
    },
    // A changed `ns` form replaces the namespace's aliases and refers.
    ReloadFixture {
        name: "ns-changed-refers",
        files: &[
            ("app/main.cljs", MAIN),
            (
                "app/core.cljs",
                r#"(ns app.core
  (:require [clojure.string :as text]))

(defn show [] (js/console.log (text/upper-case "a")))
"#,
            ),
        ],
        edit: (
            "app/core.cljs",
            r#"(ns app.core
  (:require [clojure.string :refer [upper-case join]]))

(defn show [] (js/console.log (join "-" [(upper-case "a") "b"])))
"#,
        ),
        reloads: &["app.core"],
        stdout: "A\nA-b\n",
    },
];
