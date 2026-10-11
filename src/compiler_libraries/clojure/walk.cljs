;; First-party bounded walk, using canonical persistent protocols.
(ns clojure.walk)
(declare walk prewalk postwalk prewalk-replace postwalk-replace keywordize-keys stringify-keys)

(def ^:private walk-engine (js* "((c) => {
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
})({seq: ~{seq},first: ~{first},next: ~{next},list: ~{list},apply: ~{apply},conj: ~{conj},empty: ~{empty},meta: ~{meta},withMeta: ~{with-meta},isList: ~{list?},isSeq: ~{seq?},isEntry: ~{map-entry?},isRecord: ~{record?},isColl: ~{coll?},MapEntry: ~{cljs.core/MapEntry},doall: ~{doall},map: ~{map},identity: ~{identity}})"))
(defn- walk-impl [inner outer form] (walk-engine inner outer form 0))
(defn- prewalk-impl [f form] (walk-engine f identity form 1))
(defn- postwalk-impl [f form] (walk-engine f identity form 2))
(defn- replacements [smap]
  (if (or (nil? smap) (map? smap)) smap
    (throw (js/Error. "expected replacement map or nil"))))
(defn- prewalk-replace-impl [smap form]
  (replacements smap)
  (prewalk (fn [x] (if (contains? smap x) (get smap x) x)) form))
(defn- postwalk-replace-impl [smap form]
  (replacements smap)
  (postwalk (fn [x] (if (contains? smap x) (get smap x) x)) form))
(defn- keywordize-keys-impl [m]
  (postwalk (fn [x] (if (map? x)
    (into {} (map (fn [[k v]] [(if (string? k) (keyword k) k) v]) x)) x)) m))
(defn- stringify-keys-impl [m]
  (postwalk (fn [x] (if (map? x)
    (into {} (map (fn [[k v]] [(if (keyword? k) (name k) k) v]) x)) x)) m))

;; Preserve the supported native arity boundary for saved/apply calls too.
(defn- checked-arity [f minimum maximum]
  (js* "((f, lo, hi) => function(...args) { if (args.length < lo || (hi !== null && args.length > hi)) throw new Error('Invalid arity: ' + args.length); return f(...args); })(~{f}, ~{minimum}, ~{maximum})"))
(def ^{:arglists '([inner outer form])} walk (checked-arity walk-impl 3 3))
(def ^{:arglists '([f form])} prewalk (checked-arity prewalk-impl 2 2))
(def ^{:arglists '([f form])} postwalk (checked-arity postwalk-impl 2 2))
(def ^{:arglists '([smap form])} prewalk-replace (checked-arity prewalk-replace-impl 2 2))
(def ^{:arglists '([smap form])} postwalk-replace (checked-arity postwalk-replace-impl 2 2))
(def ^{:arglists '([m])} keywordize-keys (checked-arity keywordize-keys-impl 1 1))
(def ^{:arglists '([m])} stringify-keys (checked-arity stringify-keys-impl 1 1))
