(require '[clojure.string :as str])
(def here "/private/tmp/light-cljs-phase19/")
(def profile-src "/private/tmp/light-cljs-phase13-storage-profile/src/")
(defn replace-checked [text before after]
  (assert (str/includes? text before) before)
  (str/replace text before after))
(spit (str profile-src "js_runtime.js")
      (-> (slurp (str here "runtime-before.js"))
          (replace-checked
           "    function decode(root, snapshotLimits = null) {"
           "    function decode(root, snapshotLimits = null) {
      const partNames = new Map();
      const totals = globalThis.__lightEnvironmentParts ??= Object.create(null);
      if (snapshotLimits !== null && root[0] === 'map') {
        for (let i=0; i<root[3].length; i+=2) {
          const key=root[3][i], name=(key[3] == null ? '' : key[3]+'/')+key[4];
          partNames.set(root[3][i+1], name);
        }
      }
      let partStart=0;
      const part=(name)=>totals[name]??=( {decode:0,snapshot:0,hits:0,misses:0} );")
          (replace-checked
           "        const ready = pending.pop(), d = pending.pop();"
           "        const ready = pending.pop(), d = pending.pop();
        const partName=partNames.get(d);
        if (!ready && partName != null) partStart=__lightProfileClock();")
          (replace-checked
           "          if (d[5] != null) environments.set(d[5], value);"
           "          if (d[5] != null) environments.set(d[5], value);
          if (partName != null) {const item=part(partName);item.decode+=__lightProfileClock()-partStart;item[d[0] === 'environment' ? 'hits' : 'misses']++;}")
          (replace-checked
           "      return [value, inputs.map(d => encode(values.get(d), ...snapshotLimits))];"
           "      return [value, inputs.map(d => {
        const start=__lightProfileClock(), result=encode(values.get(d), ...snapshotLimits);
        part(partNames.get(d) ?? 'other').snapshot+=__lightProfileClock()-start;
        return result;
      })];")))
(spit (str profile-src "evaluator.rs")
      (replace-checked
       (slurp (str here "evaluator-before.rs"))
       "JSON.stringify({total:globalThis.__lightReaderPreparation,parts:globalThis.__lightReaderParts})"
       "JSON.stringify({total:globalThis.__lightReaderPreparation,parts:globalThis.__lightReaderParts,environments:globalThis.__lightEnvironmentParts})"))
