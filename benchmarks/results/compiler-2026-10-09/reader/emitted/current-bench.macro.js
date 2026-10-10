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
  const symbols = new WeakMap();
  const maps = new WeakMap();
  const sets = new WeakMap();
  const cells = new WeakMap();
  const views = new WeakMap();
  const reducedValues = new WeakMap();
  const metadataValues = new WeakMap();
  const uuids = new WeakMap();
  const vars = new WeakMap();
  const queues = new WeakMap();
  const lazyValues = new WeakMap();
  const reducedPrototype = Object.freeze({});
  const sequencePrototype = Object.freeze({});
  const emptyList = Object.freeze(Object.create(sequencePrototype));
  cells.set(emptyList, Object.freeze({kind: "list", count: 0}));
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

  const symbolPrototype = Object.freeze({
    call(_self, collection, notFound) {
      if (!symbols.has(this)) throw new Error("Expected symbol");
      switch (arguments.length) {
        case 2: return lookup(collection, this, null);
        case 3: return lookup(collection, this, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    },
    toString() {
      const value = symbols.get(this);
      if (!value) throw new Error("Expected symbol");
      return value.spelling;
    }
  });

  function symbol(namespace, name) {
    const value = Object.create(symbolPrototype);
    symbols.set(value, Object.freeze({namespace, name,
      spelling: namespace == null ? name : namespace + "/" + name}));
    return Object.freeze(value);
  }

  function symbolCore(namespace, name) {
    switch (arguments.length) {
      case 1: {
        if (symbols.has(namespace)) return namespace;
        if (vars.has(namespace)) return vars.get(namespace).symbol;
        const kw = keywords.get(namespace);
        if (kw) namespace = kw.spelling;
        if (typeof namespace !== "string") throw new Error("no conversion to symbol");
        const slash = namespace.indexOf("/");
        return slash < 1 ? symbol(null, namespace)
          : symbol(namespace.slice(0, slash), namespace.slice(slash + 1));
      }
      case 2:
        if ((namespace != null && typeof namespace !== "string") || typeof name !== "string")
          throw new Error("symbol expects strings and an optional nil namespace");
        return symbol(namespace, name);
      default: return invalidArity(arguments.length);
    }
  }

  function cons(head, tail, kind) {
    const value = Object.create(sequencePrototype);
    cells.set(value, Object.freeze({kind, head, tail, count: count(tail) + 1}));
    return Object.freeze(value);
  }

  function listFromArray(items) {
    let result = emptyList;
    for (let i = items.length - 1; i >= 0; i--) result = cons(items[i], result, "list");
    return result;
  }

  function list(...items) { return listFromArray(items); }

  function view(storage, offset, indexed) {
    const value = Object.create(sequencePrototype);
    views.set(value, Object.freeze({storage, offset, indexed}));
    return Object.freeze(value);
  }

  function seq(value) {
    if (value == null || cells.get(value)?.count === 0) return null;
    const lazy = lazyValues.get(value);
    if (lazy) {
      if (lazy.thunk) { const result = lazy.thunk(); lazy.value = result; lazy.thunk = null; }
      return seq(lazy.value);
    }
    const queue = queues.get(value);
    if (queue) return queue.length ? view(queue, 0, false) : null;
    if (Array.isArray(value)) return value.length ? view(value, 0, true) : null;
    if (cells.has(value) || views.has(value)) return value;
    const items = vectors.get(value);
    if (items) return items.length ? view(items, 0, items.length <= 32) : null;
    if (typeof value === "string") return value.length ? view(value, 0, true) : null;
    const entries = maps.get(value);
    if (entries) return entries.length
      ? view(Object.freeze(entries.map(entry => vector(entry.key, entry.value))), 0, false) : null;
    const members = sets.get(value);
    if (members) return members.length
      ? view(Object.freeze(members.map(entry => entry.key)), 0, false) : null;
    throw new Error("seq not supported on this type");
  }

  function first(value) {
    const current = seq(value);
    if (current === null) return null;
    const cell = cells.get(current);
    if (cell) return cell.head;
    const slice = views.get(current);
    return slice.storage[slice.offset];
  }

  function rest(value) {
    const current = seq(value);
    if (current === null) return emptyList;
    const cell = cells.get(current);
    if (cell) return cell.tail;
    const slice = views.get(current);
    return slice.offset + 1 < slice.storage.length
      ? view(slice.storage, slice.offset + 1, slice.indexed) : emptyList;
  }

  function next(value) { return seq(rest(value)); }

  function reduced(value) {
    const marker = Object.create(reducedPrototype);
    reducedValues.set(marker, value);
    return Object.freeze(marker);
  }

  function isReduced(value) { return reducedValues.has(value); }
  function unreduced(value) { return isReduced(value) ? reducedValues.get(value) : value; }
  function deref(value) {
    if (vars.has(value)) return vars.get(value).get();
    if (!isReduced(value)) throw new Error("value does not support deref");
    return reducedValues.get(value);
  }

  function add(x, y) {
    if (arguments.length === 0) return 0;
    if (arguments.length === 1) return x;
    let result = x + y;
    for (let i = 2; i < arguments.length; i++) result = result + arguments[i];
    return result;
  }

  function reduce(rf, init, collection) {
    const arity = arguments.length;
    if (arity !== 2 && arity !== 3) return invalidArity(arity);
    let initialized = arity === 3, result = init;
    let current = seq(initialized ? collection : init);
    let slice = views.get(current), offset = slice?.offset;
    // Traverse immutable storage directly. No recursive calls, per-step seq
    // wrappers, captured closures or persistent traversal caches are needed.
    while (current !== null) {
      const cell = cells.get(current);
      const item = cell ? cell.head : slice.storage[offset];
      if (!initialized) {
        result = item;
        initialized = true;
      } else {
        // Read .call on every invocation, after the source/arguments have been
        // evaluated. Callable data and user functions share this ordinary ABI.
        result = rf.call(null, result, item);
        if (isReduced(result)) return deref(result);
      }
      if (cell) {
        current = seq(cell.tail);
        slice = views.get(current);
        offset = slice?.offset;
      } else if (++offset === slice.storage.length) current = null;
    }
    return initialized ? result : rf.call(null);
  }

  function conj(...args) {
    if (args.length === 0) return vector();
    let result = args[0];
    for (let i = 1; i < args.length; i++) {
      const items = vectors.get(result);
      if (queues.has(result)) result = queueFromArray([...queues.get(result), args[i]]);
      else if (items) result = makeVector([...items, args[i]]);
      else if (maps.has(result)) result = mapConj(result, args[i]);
      else if (sets.has(result)) {
        const entries = sets.get(result).slice();
        putMember(entries, args[i]);
        result = makeSet(entries);
      }
      else if (result == null) result = cons(args[i], emptyList, "list");
      else if (cells.has(result) || views.has(result))
        result = cons(args[i], result, cells.get(result)?.kind === "list" ? "list" : "seq");
      else throw new Error("conj not supported on this type");
    }
    return result;
  }

  function assoc(value, key, item, ...pairs) {
    if (value == null || maps.has(value)) {
      const entries = value == null ? [] : maps.get(value).slice();
      putEntry(entries, key, item);
      for (let i = 0; i < pairs.length; i += 2)
        putEntry(entries, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
      return makeMap(entries);
    }
    const original = vectors.get(value);
    if (!original) throw new Error("assoc not supported on this type");
    const items = original.slice();
    const assign = (index, value) => {
      if (typeof index !== "number") throw new Error("Vector key must be a number");
      if (index >= 0 && index < items.length) items[Math.trunc(index)] = value;
      else if (index === items.length) items.push(value);
      else outOfBounds();
    };
    // The fixed arity follows JS argument binding, including missing item.
    assign(key, item);
    for (let i = 0; i < pairs.length; i += 2)
      assign(pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
    return makeVector(items);
  }

  const mapPrototype = Object.freeze({
    call(_self, key, notFound) {
      if (!maps.has(this)) throw new Error("Expected map");
      switch (arguments.length) {
        case 2: return lookup(this, key, null);
        case 3: return lookup(this, key, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    }
  });

  function makeMap(entries) {
    const value = Object.create(mapPrototype);
    maps.set(value, Object.freeze(entries));
    return Object.freeze(value);
  }

  function entryIndex(entries, key, keyHash) {
    return entries.findIndex(entry => entry.hash === keyHash && equalPair(entry.key, key));
  }

  // Builders mutate only fresh unpublished arrays. Entry cells and retained
  // equivalent keys are shared by subsequent immutable versions.
  function putEntry(entries, key, value) {
    const keyHash = hash(key), index = entryIndex(entries, key, keyHash);
    const entry = Object.freeze({key: index < 0 ? key : entries[index].key, value, hash: keyHash});
    if (index < 0) entries.push(entry);
    else entries[index] = entry;
  }

  function mapFromArray(pairs) {
    if (pairs.length % 2) throw new Error("hash-map requires an even number of arguments");
    const entries = [];
    for (let i = 0; i < pairs.length; i += 2) putEntry(entries, pairs[i], pairs[i + 1]);
    return makeMap(entries);
  }

  function hashMap(...pairs) { return mapFromArray(pairs); }

  function dissoc(value, ...keys) {
    if (arguments.length === 0) return null;
    if (keys.length === 0 || value == null) return value;
    const original = maps.get(value);
    if (!original) throw new Error("dissoc not supported on this type");
    const entries = original.slice();
    for (const key of keys) {
      const index = entryIndex(entries, key, hash(key));
      if (index >= 0) entries.splice(index, 1);
    }
    return makeMap(entries);
  }

  function mapConj(value, member) {
    if (member == null) return value;
    const items = vectors.get(member);
    if (items) {
      if (items.length < 2) throw new Error("Map entry vector requires at least two values");
      return assoc(value, items[0], items[1]);
    }
    const entries = maps.get(value).slice();
    for (let current = seq(member); current !== null; current = next(current)) {
      const pair = vectors.get(first(current));
      if (!pair || pair.length < 2) throw new Error("Map conj requires vector entries");
      putEntry(entries, pair[0], pair[1]);
    }
    return makeMap(entries);
  }

  const setPrototype = Object.freeze({
    call(_self, key, notFound) {
      if (!sets.has(this)) throw new Error("Expected set");
      switch (arguments.length) {
        case 2: return lookup(this, key, null);
        case 3: return lookup(this, key, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    }
  });

  function makeSet(entries) {
    const value = Object.create(setPrototype);
    sets.set(value, Object.freeze(entries));
    return Object.freeze(value);
  }

  function putMember(entries, key) {
    const keyHash = hash(key);
    if (entryIndex(entries, key, keyHash) < 0)
      entries.push(Object.freeze({key, hash: keyHash}));
  }

  function setFromArray(items) {
    const entries = [];
    for (const item of items) putMember(entries, item);
    return makeSet(entries);
  }

  function hashSet(...items) { return setFromArray(items); }

  function disj(value, ...keys) {
    if (arguments.length === 0) return null;
    if (keys.length === 0) return value;
    if (value == null) return null;
    const original = sets.get(value);
    if (!original) throw new Error("disj not supported on this type");
    const entries = original.slice();
    for (const key of keys) {
      const index = entryIndex(entries, key, hash(key));
      if (index >= 0) entries.splice(index, 1);
    }
    return makeSet(entries);
  }

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
        const sym = symbols.get(namespace);
        if (sym) return makeKeyword(sym.namespace, sym.name, sym.spelling);
        if (typeof namespace !== "string") return null;
        if (namespace === "/") return keyword(null, namespace);
        const parts = namespace.split("/");
        return makeKeyword(parts.length === 2 ? parts[0] : null,
          parts.length === 2 ? parts[1] : parts[0], namespace);
      }
      case 2: {
        const nsKeyword = keywords.get(namespace) || symbols.get(namespace);
        const nameKeyword = keywords.get(name) || symbols.get(name);
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
    const named = keywords.get(value) || symbols.get(value);
    if (named) return named.name;
    if (typeof value === "string") return value;
    throw new Error("Doesn't support name on this type");
  }

  function namespace(value) {
    const named = keywords.get(value) || symbols.get(value);
    if (named) return named.namespace;
    throw new Error("Doesn't support namespace on this type");
  }

  function count(value) {
    if (lazyValues.has(value)) return toArray(value).length;
    if (queues.has(value)) return queues.get(value).length;
    if (Array.isArray(value)) return value.length;
    if (value == null) return 0;
    const items = vectors.get(value);
    if (items) return items.length;
    if (typeof value === "string") return value.length;
    const entries = maps.get(value);
    if (entries) return entries.length;
    const members = sets.get(value);
    if (members) return members.length;
    const cell = cells.get(value);
    if (cell) return cell.count ?? toArray(value).length;
    const slice = views.get(value);
    if (slice) return slice.storage.length - slice.offset;
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
    if (cells.has(value) || views.has(value)) {
      // Lists traverse to an exact zero. Indexed finite views retain the
      // pinned absolute-offset lookup, including fractional/missing slots.
      let current = value, remaining = index;
      if (!views.get(current)?.indexed && remaining < 0)
        return fallback === missing ? outOfBounds() : fallback;
      while (current !== null) {
        const slice = views.get(current);
        if (slice?.indexed) {
          const absolute = slice.offset + remaining;
          if (absolute >= 0 && absolute < slice.storage.length) return slice.storage[absolute];
          break;
        }
        if (remaining === 0 && current !== emptyList) return first(current);
        current = next(current);
        remaining--;
      }
      return fallback === missing ? outOfBounds() : fallback;
    }
    throw new Error("nth not supported on this type");
  }

  function lookup(value, key, notFound) {
    const members = sets.get(value);
    if (members) {
      const index = entryIndex(members, key, hash(key));
      return index < 0 ? notFound : members[index].key;
    }
    const entries = maps.get(value);
    if (entries) {
      const index = entryIndex(entries, key, hash(key));
      return index < 0 ? notFound : entries[index].value;
    }
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
    if (left instanceof Date) return right instanceof Date && left.valueOf() === right.valueOf();
    if (uuids.has(left)) return uuids.has(right) && uuids.get(left) === uuids.get(right);
    if (vars.has(left)) return vars.has(right) && equalPair(vars.get(left).symbol, vars.get(right).symbol);
    const leftKeyword = keywords.get(left);
    if (leftKeyword) {
      const rightKeyword = keywords.get(right);
      return !!rightKeyword && leftKeyword.spelling === rightKeyword.spelling;
    }
    const leftSymbol = symbols.get(left);
    if (leftSymbol) {
      const rightSymbol = symbols.get(right);
      return !!rightSymbol && leftSymbol.spelling === rightSymbol.spelling;
    }
    const leftMap = maps.get(left);
    if (leftMap) {
      const rightMap = maps.get(right);
      if (!rightMap || leftMap.length !== rightMap.length) return false;
      return leftMap.every(entry => {
        const index = entryIndex(rightMap, entry.key, entry.hash);
        return index >= 0 && equalPair(entry.value, rightMap[index].value);
      });
    }
    const leftSet = sets.get(left);
    if (leftSet) {
      const rightSet = sets.get(right);
      return !!rightSet && leftSet.length === rightSet.length &&
        leftSet.every(entry => entryIndex(rightSet, entry.key, entry.hash) >= 0);
    }
    if (sequential(left)) {
      if (!sequential(right) || count(left) !== count(right)) return false;
      let a = seq(left), b = seq(right);
      while (a !== null) {
        if (!equalPair(first(a), first(b))) return false;
        a = next(a); b = next(b);
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

  function sequential(value) {
    return vectors.has(value) || cells.has(value) || views.has(value) || queues.has(value) || lazyValues.has(value);
  }

  const identityHashes = new WeakMap();
  let identityCounter = 0;
  function stringHash(value) {
    let result = 0;
    for (let i = 0; i < value.length; i++) result = (Math.imul(result, 31) + value.charCodeAt(i)) | 0;
    return result;
  }

  // Internal hash contract is equality consistency, not CLJS hash integers.
  // Weak identity bookkeeping never reads or mutates foreign object fields.
  function hash(value) {
    if (value == null) return 0;
    if (typeof value === "boolean") return value ? 1231 : 1237;
    if (typeof value === "number") return stringHash(String(value));
    if (typeof value === "string") return stringHash(value);
    if (value instanceof Date) return stringHash(String(value.valueOf()));
    if (uuids.has(value)) return stringHash(uuids.get(value));
    if (vars.has(value)) return hash(vars.get(value).symbol);
    const named = keywords.get(value) || symbols.get(value);
    if (named) return stringHash((keywords.has(value) ? ":" : "'") + named.spelling);
    const entries = maps.get(value);
    if (entries) {
      let result = 0;
      for (const entry of entries) result = (result + (entry.hash ^ hash(entry.value))) | 0;
      return result;
    }
    const members = sets.get(value);
    if (members) {
      let result = 0;
      for (const entry of members) result = (result + entry.hash) | 0;
      return result;
    }
    if (sequential(value)) {
      let result = 1;
      for (let current = seq(value); current !== null; current = next(current))
        result = (Math.imul(result, 31) + hash(first(current))) | 0;
      return result;
    }
    if (!identityHashes.has(value)) identityHashes.set(value, ++identityCounter);
    return identityHashes.get(value);
  }

  // Private test adapter. Production emission keeps this factory local; only
  // the observation harness registers inspect. Exposed storage remains frozen.
  function inspect(value) {
    if (isReduced(value) || uuids.has(value) || vars.has(value)) return Object.freeze({kind: "opaque"});
    const named = keywords.get(value) || symbols.get(value);
    if (named) return Object.freeze({kind: keywords.has(value) ? "keyword" : "symbol",
      namespace: named.namespace, name: named.name, spelling: named.spelling});
    const items = vectors.get(value);
    if (items) return Object.freeze({kind: "vector", items});
    const entries = maps.get(value);
    if (entries) return Object.freeze({kind: "map", entries: Object.freeze(entries.map(
      entry => Object.freeze([entry.key, entry.value]))), count: entries.length});
    const members = sets.get(value);
    if (members) return Object.freeze({kind: "set",
      items: Object.freeze(members.map(entry => entry.key)), count: members.length});
    if (cells.has(value) || views.has(value)) {
      const items = [];
      for (let current = seq(value); current !== null; current = next(current)) items.push(first(current));
      return Object.freeze({kind: cells.get(value)?.kind || "seq", items: Object.freeze(items)});
    }
    return null;
  }

  function queueFromArray(items) {
    const value = Object.create(sequencePrototype);
    queues.set(value, Object.freeze(items.slice()));
    return Object.freeze(value);
  }

  function jsObject(pairs) {
    const result = {};
    for (let i = 0; i < pairs.length; i += 2) {
      // Literal __proto__ is an own data property, never a prototype mutation.
      Object.defineProperty(result, pairs[i], {value: pairs[i + 1], writable: true, enumerable: true, configurable: true});
    }
    return result;
  }

  function meta(value) { return metadataValues.get(value) ?? null; }
  function withMeta(value, metadata) {
    if (value == null) return null;
    let result;
    if (typeof value === "function") result = function (...args) { return value.apply(this, args); };
    else {
      const stores = [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars];
      if (!stores.some(store => store.has(value))) throw new Error("value does not support metadata");
      result = Object.create(Object.getPrototypeOf(value));
      for (const store of stores) if (store.has(value)) store.set(result, store.get(value));
    }
    metadataValues.set(result, metadata);
    return Object.freeze(result);
  }
  const uuidPrototype = Object.freeze({toString() { return uuids.get(this); }});
  function uuid(text) {
    if (typeof text !== "string") throw new Error("UUID expects a string");
    const value = Object.create(uuidPrototype);
    uuids.set(value, text.toLowerCase());
    return Object.freeze(value);
  }
  function makeVar(get, symbol, metadata = null) {
    const value = Object.create(varPrototype);
    vars.set(value, Object.freeze({get, symbol}));
    metadataValues.set(value, into(mapFromArray([keyword(null, "ns"), symbolCore(namespace(symbol)), keyword(null, "name"), symbolCore(name(symbol))]), metadata));
    return Object.freeze(value);
  }
  const varPrototype = Object.freeze({
    call(_self, ...args) { return vars.get(this).get().call(null, ...args); },
    toString() { return "#'" + vars.get(this).symbol; }
  });
  function rePattern(pattern) {
    if (pattern instanceof RegExp) return pattern;
    if (typeof pattern !== "string") throw new TypeError("regex pattern requires a string");
    const prefix = /^\(\?([idmsux]*)\)/.exec(pattern);
    return new RegExp(prefix ? pattern.slice(prefix[0].length) : pattern, prefix ? prefix[1] : "");
  }
  function reMatch(re, text, full) {
    if (typeof text !== "string") throw new TypeError("regex matching requires a string");
    const result = re.exec(text);
    if (result === null || (full && result[0] !== text)) return null;
    return result.length === 1 ? result[0] : vectorFromArray(Array.from(result));
  }
  function lazySequence(thunk) {
    const value = Object.create(sequencePrototype);
    lazyValues.set(value, {thunk, value: null});
    return Object.freeze(value);
  }
  function reSeq(re, text) {
    if (typeof text !== "string") throw new TypeError("regex matching requires a string");
    const match = re.exec(text);
    if (match === null) return null;
    const value = match.length === 1 ? match[0] : vectorFromArray(Array.from(match));
    // A lazy tail must not compute a count (or execute the next RegExp.exec).
    const tail = lazySequence(() => {
      const offset = match.index + Math.max(1, match[0].length);
      return offset <= text.length ? reSeq(re, text.slice(offset)) : null;
    });
    const result = Object.create(sequencePrototype);
    cells.set(result, Object.freeze({kind: "seq", head: value, tail}));
    return Object.freeze(result);
  }
  function toArray(value) {
    const result = [];
    for (let current = seq(value); current !== null; current = next(current)) result.push(first(current));
    return result;
  }
  function vec(value) { return vectors.has(value) ? value : vectorFromArray(toArray(value)); }
  function concat(...values) {
    function step(index, current) {
      return lazySequence(() => {
        let items = seq(current);
        while (items === null && index < values.length) items = seq(values[index++]);
        if (items === null) return null;
        const result = Object.create(sequencePrototype);
        cells.set(result, Object.freeze({kind: "seq", head: first(items), tail: step(index, rest(items))}));
        return Object.freeze(result);
      });
    }
    return step(0, null);
  }
  function sequence(value) { return seq(value) ?? emptyList; }
  function apply(fn, ...args) {
    if (args.length === 0) return invalidArity(1);
    const tail = args.pop();
    return fn.call(null, ...args, ...toArray(tail));
  }
  function into(to, from) {
    for (let current = seq(from); current !== null; current = next(current)) to = conj(to, first(current));
    return to;
  }
  function print(value, readable) {
    if (value == null) return "nil";
    if (typeof value === "string") return readable ? JSON.stringify(value) : value;
    if (typeof value === "number") return Number.isNaN(value) ? "##NaN" : value === Infinity ? "##Inf" : value === -Infinity ? "##-Inf" : Object.is(value, -0) ? "-0.0" : String(value);
    if (typeof value === "boolean") return String(value);
    if (keywords.has(value) || symbols.has(value) || vars.has(value)) return String(value);
    if (uuids.has(value)) return '#uuid "' + uuids.get(value) + '"';
    if (value instanceof Date) return '#inst "' + value.toISOString().replace(/Z$/, "-00:00") + '"';
    if (value instanceof RegExp) return '#"' + value.source + '"';
    const items = vectors.get(value) || queues.get(value) || (sequential(value) ? toArray(value) : null);
    if (items) {
      const [start, end] = queues.has(value) ? ["#queue [", "]"] : vectors.has(value) ? ["[", "]"] : ["(", ")"];
      return start + items.map(item => print(item, readable)).join(" ") + end;
    }
    const entries = maps.get(value);
    if (entries) return "{" + entries.map(e => print(e.key, readable) + " " + print(e.value, readable)).join(", ") + "}";
    const members = sets.get(value);
    if (members) return "#{" + members.map(e => print(e.key, readable)).join(" ") + "}";
    if (Array.isArray(value)) return "#js [" + value.map(v => print(v, readable)).join(" ") + "]";
    if (typeof value === "object") return "#js {" + Object.keys(value).map(k => (/^[A-Za-z_*+?!\-'][\w*+?!\-']*$/.test(k) ? ":" + k : JSON.stringify(k)) + " " + print(value[k], readable)).join(", ") + "}";
    return String(value);
  }
  function prStr(...values) { return values.map(value => print(value, true)).join(" "); }
  function str(...values) { return values.map(value => value == null ? "" : typeof value === "string" ? value : sequential(value) || maps.has(value) || sets.has(value) ? print(value, true) : String(value)).join(""); }

  const core = Object.freeze({keyword: keywordCore, symbol: symbolCore, vector, list,
    hashMap, hashSet, dissoc, disj, conj, assoc, seq, first, rest, next, name, namespace, count, nth, get, contains, equal,
    reduce, add, reduced, isReduced, unreduced, deref, meta, withMeta, prStr, str, uuid,
    identical: (a, b) => a === b, isUuid: value => uuids.has(value), isInst: value => value instanceof Date,
    instMs: value => value.getTime(), isVar: value => vars.has(value),
    rePattern, reSeq, reFind: (re, text) => reMatch(re, text, false), reMatches: (re, text) => reMatch(re, text, true),
    isRegex: value => value instanceof RegExp, vec, concat, sequence, apply, into});
  return Object.freeze({keyword, symbol, vector, vectorFromArray, listFromArray, mapFromArray, setFromArray, queueFromArray, jsObject, makeVar, core, inspect, hash});
})();
  return (rt.core.reduce).call(null, rt.core.add, 0, rt.vectorFromArray([1, 2, 3]));
})()
