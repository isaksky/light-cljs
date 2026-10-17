(require '[clojure.string :as str])
(def here "/private/tmp/light-cljs-phase24/")
(def roots (slurp "src/compiler_roots.js"))
(defn replace-checked [text before after]
  (assert (str/includes? text before) before)
  (str/replace text before after))
(def helpers "  const readyExtensions = [];
  // First initialized extension after this declaration order.
  function readyAfter(order) {
    let low = 0, high = readyExtensions.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (readyExtensions[middle].order <= order) low = middle + 1;
      else high = middle;
    }
    return low;
  }
  function changesFrontier(next) {
    const low = Math.min(frontier, next), high = Math.max(frontier, next);
    const index = readyAfter(low);
    return index < readyExtensions.length && readyExtensions[index].order <= high;
  }
")
(def revised
  (-> roots
      (replace-checked "  let frontier = Infinity, declarationFrontier = Infinity;"
                       (str "  let frontier = Infinity, declarationFrontier = Infinity;\n" helpers))
      (replace-checked "      bundle.state = 'ready';"
                       "      bundle.state = 'ready';\n      if (bundle.indices.length) readyExtensions.splice(readyAfter(bundle.order), 0, bundle);")
      (replace-checked "bundles.some(bundle => bundle.indices.length && bundle.state === 'ready' && (bundle.order <= frontier) !== (bundle.order <= next))"
                       "changesFrontier(next)")
      (replace-checked "    for (let i = bundles.length - 1; i >= 0; i--) {\n      const bundle = bundles[i];\n      if (bundle.indices.length && bundle.state === 'ready' && bundle.order <= frontier)"
                       "    for (let i = readyExtensions.length - 1; i >= 0; i--) {\n      const bundle = readyExtensions[i];\n      if (bundle.order <= frontier)")
      (replace-checked "for (const bundle of bundles) if (bundle.indices.length && bundle.state === 'ready' && bundle.order <= frontier)"
                       "for (const bundle of readyExtensions) if (bundle.order <= frontier)")
      (replace-checked "bundles.flatMap(bundle => bundle.indices.length && bundle.state === 'ready' ? __lightRuntimeForCompiler.snapshotDeclarationPatches(bundle.patches) : [])"
                       "readyExtensions.flatMap(bundle => __lightRuntimeForCompiler.snapshotDeclarationPatches(bundle.patches))")))
(spit (str here "before.js") roots)
(spit (str here "after.js") revised)
