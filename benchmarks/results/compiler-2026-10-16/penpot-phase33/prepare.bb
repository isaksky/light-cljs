(require '[babashka.fs :as fs] '[clojure.string :as str])
(def here (fs/parent *file*))
(def project (fs/file "/private/tmp/light-cljs-phase21/project"))
(doseq [path ["Cargo.toml" "Cargo.lock" "build.rs"]]
  (fs/copy path (fs/file project path) {:replace-existing true}))
(doseq [path ["src" "vendor" "benches" "tests" "examples"]]
  (fs/copy-tree path (fs/file project path) {:replace-existing true}))
(defn replace! [file before after]
  (let [path (fs/file project file) text (slurp path)]
    (assert (str/includes? text before) (str "Missing anchor " file " " before))
    (spit path (str/replace text before after))))
(replace! "src/js_runtime.js" "const borrowed = new WeakMap(), literals = new WeakMap(), primitives = new Map(), environments = new Map();"
          "const borrowed = new WeakMap(), literals = new WeakMap(), primitives = new Map(), environments = new Map(), macroInputs = new WeakMap();")
(replace! "src/js_runtime.js" "function decode(root, snapshotLimits = null) {" "function decode(root, snapshotLimits = null, macroInput = false) {")
(replace! "src/js_runtime.js" "      const value = values.get(root);\n      if (snapshotLimits === null) return value;"
          "      const value = values.get(root);\n      if (macroInput) macroInputs.set(value, {root, values});\n      if (snapshotLimits === null) return value;")
(replace! "src/js_runtime.js" "function decodeNode(d, values) {" "function decodeNode(d, values, reuse = false) {")
(replace! "src/js_runtime.js" "      let value;\n      switch (kind) {\n        case \"environment\":"
          "      let value;\n      if (reuse) value = values.get(d);\n      else switch (kind) {\n        case \"environment\":")
(replace! "src/js_runtime.js" "    function encode(value, limit, depthLimit, unseen = null) {"
          (str (slurp (fs/file here "bridge.js")) "\n    function encode(value, limit, depthLimit, unseen = null) {"))
(replace! "src/js_runtime.js" "return Object.freeze({version: 4, decode, encode,"
          "return Object.freeze({version: 5, decode, encode, macroArguments, releaseMacroInput: value => macroInputs.delete(value),")
(replace! "src/form_bridge.rs" "pub const FORM_BRIDGE_VERSION: u32 = 4;" "pub const FORM_BRIDGE_VERSION: u32 = 5;")
(replace! "src/evaluator.rs" "                importForm: descriptor => save(bridge.decode(descriptor)),"
          (str "                importForm: descriptor => save(bridge.decode(descriptor)),\n"
               "                importMacroForm: descriptor => save(bridge.decode(descriptor, null, true)),\n"
               "                macroArguments: (form, nodes, depth) => {{\n"
               "                  const [inputs, snapshots] = bridge.macroArguments(get(form), nodes, depth), ids = [];\n"
               "                  try {{ for (const value of inputs) ids.push(save(value)); return [ids, snapshots]; }}\n"
               "                  catch (error) {{ for (const id of ids) values.delete(id); throw error; }}\n"
               "                }},"))
(replace! "src/evaluator.rs" "release: id => {{ get(id); values.delete(id); }}"
          "release: id => {{ bridge.releaseMacroInput(get(id)); values.delete(id); }}")
(replace! "src/evaluator.rs" "    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError>;"
          (str "    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError>;\n"
               "    fn import_macro_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> { self.import_form(form, limits) }\n"
               "    fn macro_arguments(&self, _form: ValueId, call: &Form, limits: ConversionLimits) -> Result<Vec<ValueId>, EvalError> {\n"
               "        let mut values = Vec::new();\n"
               "        for argument in call.children().iter().skip(1) {\n"
               "            match self.import_form(argument, limits) {\n"
               "                Ok(value) => values.push(value),\n"
               "                Err(error) => { for value in values { let _ = self.release(value); } return Err(error); }\n"
               "            }\n"
               "        }\n        Ok(values)\n    }"))
(replace! "src/evaluator.rs" "        Evaluator::import_form(self, form, limits)\n    }"
          (str "        Evaluator::import_form(self, form, limits)\n    }\n"
               "    fn import_macro_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {\n"
               "        self.with_active(|active| { active.bridge.borrow_mut().retain(form); active.import_data_with_service(&form.data(), limits, \"importMacroForm\") })\n    }\n"
               "    fn macro_arguments(&self, form: ValueId, _call: &Form, limits: ConversionLimits) -> Result<Vec<ValueId>, EvalError> {\n"
               "        self.with_active(|active| active.macro_arguments(form, limits))\n    }"))
(replace! "src/evaluator.rs" "        ActiveEvaluator::import_form(self, form, limits)\n    }"
          (str "        ActiveEvaluator::import_form(self, form, limits)\n    }\n"
               "    fn import_macro_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {\n"
               "        self.bridge.borrow_mut().retain(form); self.import_data_with_service(&form.data(), limits, \"importMacroForm\")\n    }\n"
               "    fn macro_arguments(&self, form: ValueId, _call: &Form, limits: ConversionLimits) -> Result<Vec<ValueId>, EvalError> {\n"
               "        ActiveEvaluator::macro_arguments(self, form, limits)\n    }"))
(replace! "src/evaluator.rs" "        let index = self.enter(|ctx| {\n            let descriptor = self\n                .bridge\n                .borrow_mut()\n                .encode(&ctx, data, self.control, limits)?;\n            let f: Function = self.services.get(\"importForm\")?;"
          (str "        self.import_data_with_service(data, limits, \"importForm\")\n    }\n"
               (slurp (fs/file here "active.rs"))
               "    fn import_data_with_service(&self, data: &Datum, limits: ConversionLimits, service: &str) -> Result<ValueId, EvalError> {\n"
               "        let index = self.enter(|ctx| {\n            let descriptor = self\n                .bridge\n                .borrow_mut()\n                .encode(&ctx, data, self.control, limits)?;\n            let f: Function = self.services.get(service)?;"))
(replace! "src/macro_host.rs" "args.push(evaluator.import_form(call, limits)?);" "args.push(evaluator.import_macro_form(call, limits)?);")
(replace! "src/macro_host.rs" "                for argument in call.children().iter().skip(1) {\n                    args.push(evaluator.import_form(argument, limits)?);\n                }"
          "                args.extend(evaluator.macro_arguments(args[0], call, limits)?);")
(println "Prepared phase33 prototype" project)
