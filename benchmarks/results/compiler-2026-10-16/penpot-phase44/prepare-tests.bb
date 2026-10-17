(require '[clojure.string :as str])
(def file "/private/tmp/light-cljs-phase44/project/src/compiler_bootstrap.rs")
(def original (slurp file))
(def pairs [[".install_unit_service(\"cancelDemand\", move |_, _| {" ".clone().install_compiler_service(\"cancelDemand\", Rc::new(move |_, _| {"]
            [".install_unit_service(\"cancelTarget\", move |_, _| {" ".clone().install_compiler_service(\"cancelTarget\", Rc::new(move |_, _| {"]
            ["Ok(\"(rt) => 42\".into())\n                })" "Ok(\"(rt) => 42\".into())\n                }))"]])
(def changed (reduce (fn [text [old new]] (assert (str/includes? text old) old) (str/replace text old new)) original pairs))
(spit file changed)
(println "portable cancellation callbacks for internal bootstrap tests")
