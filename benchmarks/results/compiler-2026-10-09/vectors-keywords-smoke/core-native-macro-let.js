(() => {
  "use strict";
  const truth = (value) => value != null && value !== false;
  const rt = // Original bounded runtime for emitted JavaScript. This file is a factory
// expression, embedded once in each compilation unit that needs owned values.
(() => {
  "use strict";

  // Brands and payloads stay private: copying fields or inheriting a value's
  // prototype cannot turn a host object into a language value.
  const keywords = new WeakMap();
  const vectors = new WeakMap();
  const missing = Object.freeze({});
  const invalidArity = n => { throw new Error("Invalid arity: " + n); };
  const outOfBounds = () => { throw new Error("Index out of bounds"); };

  const keywordPrototype = Object.freeze({
    call(_self, collection, notFound) {
      if (!keywords.has(this)) throw new Error("Expected keyword");
      switch (arguments.length) {
        case 2: return lookup(collection, this, null);
        case 3: return lookup(collection, this, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    },
    toString() {
      const value = keywords.get(this);
      if (!value) throw new Error("Expected keyword");
      return ":" + value.spelling;
    }
  });

  const vectorPrototype = Object.freeze({
    // The pinned fixed method accepts one index and ignores extra arguments.
    call(_self, index) {
      const items = vectors.get(this);
      if (!items) throw new Error("Expected vector");
      if (typeof index !== "number") throw new Error("Key must be integer");
      return vectorNth(items, index, missing);
    }
  });

  function makeKeyword(namespace, name, spelling) {
    const value = Object.create(keywordPrototype);
    keywords.set(value, Object.freeze({namespace, name, spelling}));
    return Object.freeze(value);
  }

  // Literal construction receives separately owned namespace/name strings.
  function keyword(namespace, name) {
    return makeKeyword(namespace, name,
      namespace == null ? name : namespace + "/" + name);
  }

  function makeVector(items) {
    const value = Object.create(vectorPrototype);
    vectors.set(value, Object.freeze(items));
    return Object.freeze(value);
  }

  function vector(...items) {
    return makeVector(items);
  }

  // Literal emission uses an array to avoid the host's argument-count limit.
  // Copy even when a test/host caller supplies its own array.
  function vectorFromArray(items) {
    return makeVector(Array.from(items));
  }

  function keywordCore(namespace, name) {
    switch (arguments.length) {
      case 1: {
        if (keywords.has(namespace)) return namespace;
        if (typeof namespace !== "string") return null;
        if (namespace === "/") return keyword(null, namespace);
        const parts = namespace.split("/");
        return makeKeyword(parts.length === 2 ? parts[0] : null,
          parts.length === 2 ? parts[1] : parts[0], namespace);
      }
      case 2: {
        const nsKeyword = keywords.get(namespace);
        const nameKeyword = keywords.get(name);
        if (nsKeyword) namespace = nsKeyword.name;
        if (nameKeyword) name = nameKeyword.name;
        if ((namespace != null && typeof namespace !== "string") ||
            typeof name !== "string") {
          throw new Error("keyword expects strings or keywords and an optional nil namespace");
        }
        return keyword(namespace, name);
      }
      default: return invalidArity(arguments.length);
    }
  }

  function name(value) {
    const named = keywords.get(value);
    if (named) return named.name;
    if (typeof value === "string") return value;
    throw new Error("Doesn't support name on this type");
  }

  function namespace(value) {
    const named = keywords.get(value);
    if (named) return named.namespace;
    throw new Error("Doesn't support namespace on this type");
  }

  function count(value) {
    if (value == null) return 0;
    const items = vectors.get(value);
    if (items) return items.length;
    if (typeof value === "string") return value.length;
    throw new Error("count not supported on this type");
  }

  function vectorNth(items, index, notFound) {
    if (index >= 0 && index < items.length) return items[Math.trunc(index)];
    if (notFound === missing) return outOfBounds();
    return notFound;
  }

  function nth(value, index, notFound) {
    const arity = arguments.length;
    if (arity !== 2 && arity !== 3) return invalidArity(arity);
    if (typeof index !== "number") throw new Error("Index argument to nth must be a number");
    if (value == null) return arity === 2 ? value : notFound;
    const fallback = arity === 2 ? missing : notFound;
    const items = vectors.get(value);
    if (items) return vectorNth(items, index, fallback);
    if (typeof value === "string") {
      if (index > -1 && index < value.length) return value.charAt(index | 0);
      if (fallback === missing) return outOfBounds();
      return fallback;
    }
    throw new Error("nth not supported on this type");
  }

  function lookup(value, key, notFound) {
    const items = vectors.get(value);
    if (items) return typeof key === "number" ? vectorNth(items, key, notFound) : notFound;
    if (typeof value === "string" && key != null && key > -1 && key < value.length) {
      return value.charAt(key | 0);
    }
    // Host arrays and objects remain opaque; they do not gain a collection API.
    return notFound;
  }

  function get(value, key, notFound) {
    switch (arguments.length) {
      case 2: return lookup(value, key, null);
      case 3: return lookup(value, key, notFound);
      default: return invalidArity(arguments.length);
    }
  }

  function contains(value, key) {
    const items = vectors.get(value);
    if (items) return Number.isInteger(key) && key >= 0 && key < items.length;
    return lookup(value, key, missing) !== missing;
  }

  function equalPair(left, right) {
    if (left == null) return right == null;
    if (left === right) return true;
    const leftKeyword = keywords.get(left);
    if (leftKeyword) {
      const rightKeyword = keywords.get(right);
      return !!rightKeyword && leftKeyword.spelling === rightKeyword.spelling;
    }
    const leftItems = vectors.get(left);
    if (leftItems) {
      const rightItems = vectors.get(right);
      if (!rightItems || leftItems.length !== rightItems.length) return false;
      for (let i = 0; i < leftItems.length; i++) {
        if (!equalPair(leftItems[i], rightItems[i])) return false;
      }
      return true;
    }
    // Primitive NaN stays unequal to itself; foreign objects use identity only.
    return false;
  }

  function equal(...values) {
    for (let i = 1; i < values.length; i++) {
      if (!equalPair(values[i - 1], values[i])) return false;
    }
    return true;
  }

  // Private test adapter. Production emission keeps this factory local; only
  // the observation harness registers inspect. Exposed storage remains frozen.
  function inspect(value) {
    const named = keywords.get(value);
    if (named) return Object.freeze({kind: "keyword", namespace: named.namespace,
      name: named.name, spelling: named.spelling});
    const items = vectors.get(value);
    return items ? Object.freeze({kind: "vector", items}) : null;
  }

  const core = Object.freeze({keyword: keywordCore, vector, name, namespace, count,
    nth, get, contains, equal});
  return Object.freeze({keyword, vector, vectorFromArray, core, inspect});
})();
  {
    const l0 = rt.vectorFromArray([rt.keyword("demo", "x"), 1]);
    return (rt.core.count).call(null, l0);
  }
})()
