;; First-party bounded walk, using canonical persistent protocols.
(ns clojure.walk)
(declare walk prewalk postwalk prewalk-replace postwalk-replace keywordize-keys stringify-keys)

;; The engine is made at its first call and kept in a one-element array, so
;; that a program which never walks drops this namespace at advanced: an
;; unused array definition is removable, a top-level js* expression is not.
(def ^:private walk-engine-cell (array nil))
(defn- walk-engine [inner outer form mode]
  (js* "(~{walk-engine-cell}[0] || (~{walk-engine-cell}[0] = ((c) => {
  let depth = 0, calls = 0;
  function frame(value) {
    const shape = c.isList(value) ? 'list' : c.isEntry(value) ? 'entry' : c.isSeq(value) ? 'seq' : c.isRecord(value) ? 'record' : c.isColl(value) ? 'coll' : null;
    if (shape === null) return null;
    return {shape, value, cursor: c.seq(value), values: [], result: shape === 'record' ? value : shape === 'coll' ? c.empty(value) : null};
  }
  function push(f, value) {
    if (f.shape === 'record' || f.shape === 'coll') f.result = c.conj(f.result, value);
    else f.values.push(value);
  }
  function finish(f) {
    if (f.shape === 'entry') return new c.MapEntry(f.values[0], f.values[1], null);
    if (f.shape === 'record' || f.shape === 'coll') return f.result;
    let result = c.apply(c.list, f.values);
    if (f.shape === 'seq') result = c.doall(c.map(c.identity, result));
    return c.withMeta(result, c.meta(f.value));
  }
  function walk(inner, outer, form, mode) {
    if (mode === 0) {
      const initialDepth = depth;
      try {
        if (++depth > 256) throw new Error('walk depth limit exceeded');
        const f = frame(form);
        if (!f) return outer(form);
        while (f.cursor !== null) {
          push(f, inner(c.first(f.cursor)));
          f.cursor = c.next(f.cursor);
        }
        return outer(finish(f));
      } finally { depth = initialDepth; }
    }
    const initialDepth = depth, frames = [];
    let pending = form;
    try {
      descend: while (true) {
        if (++depth > 256) throw new Error('walk depth limit exceeded');
        if (mode === 1) pending = inner(pending);
        const f = frame(pending);
        let result;
        if (f && f.cursor !== null) {
          pending = c.first(f.cursor);
          frames.push(f);
          continue;
        }
        result = f ? finish(f) : pending;
        while (true) {
          if (mode === 2) result = inner(result);
          --depth;
          if (!frames.length) return result;
          const parent = frames[frames.length - 1];
          push(parent, result);
          parent.cursor = c.next(parent.cursor);
          if (parent.cursor !== null) {
            pending = c.first(parent.cursor);
            continue descend;
          }
          frames.pop();
          result = finish(parent);
        }
      }
    } finally { depth = initialDepth; }
  }
  return function(inner, outer, form, mode) {
    if (calls >= 8) throw new Error('walk depth limit exceeded');
    ++calls;
    try { return walk(inner, outer, form, mode); }
    finally { --calls; }
  };
})({seq: ~{seq},first: ~{first},next: ~{next},list: ~{list},apply: ~{apply},conj: ~{conj},empty: ~{empty},meta: ~{meta},withMeta: ~{with-meta},isList: ~{list?},isSeq: ~{seq?},isEntry: ~{map-entry?},isRecord: ~{record?},isColl: ~{coll?},MapEntry: ~{cljs.core/MapEntry},doall: ~{doall},map: ~{map},identity: ~{identity}})))(~{inner}, ~{outer}, ~{form}, ~{mode})"))

;; Each public function has a second method that only throws: with two
;; methods it has a dispatcher, which rejects every other count with
;; "Invalid arity: N" when the call's count is wrong or the function is
;; applied or saved. A program that calls it directly with the right count
;; keeps only the first method at advanced.
(defn walk {:arglists '([inner outer form])}
  ([inner outer form] (walk-engine inner outer form 0))
  ([_ _ _ _] (throw (js/Error. "Invalid arity: 4"))))
(defn prewalk {:arglists '([f form])}
  ([f form] (walk-engine f identity form 1))
  ([_ _ _] (throw (js/Error. "Invalid arity: 3"))))
(defn postwalk {:arglists '([f form])}
  ([f form] (walk-engine f identity form 2))
  ([_ _ _] (throw (js/Error. "Invalid arity: 3"))))
(defn- replacements [smap]
  (if (or (nil? smap) (map? smap)) smap
    (throw (js/Error. "expected replacement map or nil"))))
(defn prewalk-replace {:arglists '([smap form])}
  ([smap form]
   (replacements smap)
   (prewalk (fn [x] (if (contains? smap x) (get smap x) x)) form))
  ([_ _ _] (throw (js/Error. "Invalid arity: 3"))))
(defn postwalk-replace {:arglists '([smap form])}
  ([smap form]
   (replacements smap)
   (postwalk (fn [x] (if (contains? smap x) (get smap x) x)) form))
  ([_ _ _] (throw (js/Error. "Invalid arity: 3"))))
(defn keywordize-keys {:arglists '([m])}
  ([m]
   (postwalk (fn [x] (if (map? x)
     (into {} (map (fn [[k v]] [(if (string? k) (keyword k) k) v]) x)) x)) m))
  ([_ _] (throw (js/Error. "Invalid arity: 2"))))
(defn stringify-keys {:arglists '([m])}
  ([m]
   (postwalk (fn [x] (if (map? x)
     (into {} (map (fn [[k v]] [(if (keyword? k) (name k) k) v]) x)) x)) m))
  ([_ _] (throw (js/Error. "Invalid arity: 2"))))
