// Original bounded runtime for emitted JavaScript. This file is a factory
// expression, embedded once in each compilation unit that needs owned values.
(() => {
  "use strict";

  // Brands and payloads stay private: copying fields or inheriting a value's
  // prototype cannot turn a host object into a language value.
  const keywords = new WeakMap();
  const vectors = new WeakMap();
  const symbols = new WeakMap();
  const maps = new WeakMap();
  const mapKinds = new WeakMap();
  const sortedComparators = new WeakMap();
  const mapEntries = new WeakSet();
  const sets = new WeakMap();
  const setKinds = new WeakMap();
  const transients = new WeakMap();
  const cells = new WeakMap();
  const views = new WeakMap();
  const reducedValues = new WeakMap();
  const metadataValues = new WeakMap();
  const uuids = new WeakMap();
  const uuidHashes = new WeakMap();
  const vars = new WeakMap();
  const atoms = new WeakMap(), volatiles = new WeakMap();
  const queues = new WeakMap();
  const lazyValues = new WeakMap(), delays = new WeakMap();
  const eductions = new WeakMap();
  const ifnValues = new WeakMap();
  const metaFunctions = new WeakMap();
  const typeNames = new WeakMap();
  const typeFields = new WeakMap(), recordTypes = new WeakMap(), reifyValues = new WeakSet();
  // Extension prototypes are public and mutable; immutable collection payloads
  // and their base callability remain private and frozen.
  const builtinTypes = {};
  function builtinType(name, base) {
    if (!builtinTypes[name]) {
      const Type = name === "UUID" ? function (text, cachedHash) { return uuidValue(text, cachedHash); } : name === "Delay" ? Delay : name === "LazySeq" ? LazySeq : name === "Eduction" ? Eduction : name === "ChunkBuffer" ? ChunkBuffer : name === "ArrayChunk" ? arrayChunk : name === "ChunkedCons" ? ChunkedCons : name === "IndexedSeq" ? function (array, index, metadata) { return withMeta(view(array, index, true), metadata); } : name === "PersistentTreeMap" ? treeMapConstructor : name === "PersistentTreeSet" ? treeSetConstructor : name === "PersistentTreeMapSeq" ? treeMapSeqConstructor : name === "MapEntry" ? function (key, value) { return mapEntry(key, value); } : name === "Range" || name === "IntegerRange" ? function (metadata, start, end, step, suppliedCount) { return withMeta(rangeNode(start, end, step, name === "IntegerRange", suppliedCount), metadata); } : function () {};
      Type.prototype = Object.create(base);
      Object.defineProperty(Type.prototype, "constructor", {value: Type, writable: true, configurable: true});
      builtinTypes[name] = Type;
    }
    return builtinTypes[name];
  }
  function builtinValue(name, base) { return Object.create(builtinType(name, base).prototype); }
  const reducedPrototype = Object.freeze({});
  const sequencePrototype = Object.freeze({
    equiv(other) { return equalPair(this, other); },
    toString() { return print(this, true); },
    indexOf(value, start = 0) { return sequentialIndex(this, value, start, false); },
    lastIndexOf(value, start = count(this)) { return sequentialIndex(this, value, start, true); }
  });
  const emptyList = builtinValue("EmptyList", sequencePrototype);
  cells.set(emptyList, Object.freeze({kind: "list", count: 0}));
  const missing = Object.freeze({});
  const truth = value => value != null && value !== false;
  const invalidArity = n => { throw new Error("Invalid arity: " + n); };
  const outOfBounds = () => { throw new Error("Index out of bounds"); };

  const keywordPrototype = Object.freeze({
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    call(_self, collection, notFound) {
      if (!keywords.has(this)) throw new Error("Expected keyword");
      switch (arguments.length) {
        case 2: return get(collection, this);
        case 3: return get(collection, this, notFound);
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
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    call(_self, collection, notFound) {
      if (!symbols.has(this)) throw new Error("Expected symbol");
      switch (arguments.length) {
        case 2: return get(collection, this);
        case 3: return get(collection, this, notFound);
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
    const value = builtinValue("Symbol", symbolPrototype);
    symbols.set(value, Object.freeze({namespace, name,
      spelling: namespace == null ? name : namespace + "/" + name}));
    return value;
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

  let gensymCounter = 0;
  const generatedNames = new Set();
  function gensym(prefix) {
    if (arguments.length > 1) return invalidArity(arguments.length);
    const base = str(arguments.length === 0 ? "G__" : prefix);
    let name; do { name = base + ++gensymCounter; } while (generatedNames.has(name));
    generatedNames.add(name);
    return symbolCore(name);
  }

  // Evaluators install their expansion services without replacing core or
  // coupling this runtime to a particular compiler or JavaScript engine.
  let macroexpandServices = {};
  function setMacroexpandServices(services) {
    const next = services == null ? {} : services;
    if (typeof next !== "object") throw new TypeError("Expected macroexpand services object");
    const {macroexpand, macroexpand1} = next;
    for (const service of [macroexpand, macroexpand1]) {
      if (service != null && typeof service !== "function") throw new TypeError("Expected macroexpand service function");
    }
    macroexpandServices = {macroexpand, macroexpand1};
  }
  function expansionService(name, displayName) {
    return function (form) {
      if (arguments.length !== 1) return invalidArity(arguments.length);
      const service = macroexpandServices[name];
      if (service == null) throw new Error(displayName + " service is not installed");
      return service.call(null, form);
    };
  }
  const macroexpand = expansionService("macroexpand", "macroexpand");
  const macroexpand1 = expansionService("macroexpand1", "macroexpand-1");

  function cons(head, tail, kind = "cons") {
    if (kind === "cons") {
      if (tail == null) kind = "list";
      else if (!implementsProtocol(ISeq, tail)) tail = seq(tail);
    }
    const value = builtinValue(kind === "list" ? "List" : "Cons", sequencePrototype);
    cells.set(value, Object.freeze({kind, head, tail, ...(kind === "list" ? {count: count(tail) + 1} : {})}));
    return value;
  }

  function listFromArray(items) {
    let result = emptyList;
    for (let i = items.length - 1; i >= 0; i--) result = cons(items[i], result, "list");
    return result;
  }

  function list(...items) { return listFromArray(items); }

  function view(storage, offset, indexed, kind = "indexed", tail = null) {
    const value = builtinValue(kind === "indexed" ? "IndexedSeq" : kind === "chunked" ? "ChunkedSeq" : kind === "hash-map" ? "NodeSeq" : kind === "sorted-map" ? "PersistentTreeMapSeq" : "IndexedSeq", sequencePrototype);
    views.set(value, Object.freeze({storage, offset, indexed, kind, tail}));
    return value;
  }

  function seq(value) {
    if (value == null) return null;
    const method = publicMethod(value, "ISeqable/-seq", 1, true);
    if (method) return method.call(value, value);
    if (value == null || (cells.get(value)?.kind === "list" && cells.get(value).count === 0)) return null;
    const lazy = lazyValues.get(value);
    if (lazy) {
      let current = deferredValue(lazy, true);
      // Flatten nested LazySeq results iteratively, retaining the raw final
      // value. CLJS calls seq on that raw value again on each traversal.
      while (lazyValues.has(current)) {
        current = deferredValue(lazyValues.get(current), true);
        // Compress as we advance, so discarded wrappers and their captured
        // source heads can be collected even through a long empty prefix.
        lazy.value = current;
      }
      lazy.value = current;
      return seq(current);
    }
    const queue = queues.get(value);
    if (queue) return queue.length ? view(queue, 0, false, "queue") : null;
    if (Array.isArray(value)) return value.length ? view(value, 0, true) : null;
    if (cells.has(value)) return value;
    if (views.has(value)) { const v = views.get(value); return v.offset < v.storage.length ? value : null; }
    const items = vectors.get(value);
    if (items) return items.length ? view(items, 0, items.length <= 32, items.length <= 32 ? "indexed" : "chunked") : null;
    if (typeof value === "string") return value.length ? view(value, 0, true) : null;
    const entries = maps.get(value);
    if (entries) return entries.length
      ? view(Object.freeze(entries.map(entry => {
          const pair = vector(entry.key, entry.value);
          Object.setPrototypeOf(pair, builtinType("MapEntry", vectorPrototype).prototype);
          mapEntries.add(pair);
          return pair;
        })), 0, false, mapKinds.get(value)) : null;
    const members = sets.get(value);
    if (members) return members.length
      ? view(Object.freeze(members.map(entry => entry.key)), 0, false, "set") : null;
    if (value[Symbol.iterator] != null) return iteratorSeq(value[Symbol.iterator]());
    throw new Error(String(value) + " is not ISeqable");
  }

  // CLJS reprobes a nil tail; only successful nonempty tails are memoized.
  function iteratorSeq(iterator) {
    const item = iterator.next();
    if (item.done) return null;
    return installMethods({}, [["ISeqable", 0, null], ["ISeq", 0, null],
      ["ISeqable/-seq", 1, function () { return this; }],
      ["ISeq/-first", 1, () => item.value],
      ["ISeq/-rest", 1, (() => { let tail = null; return () => {
        if (tail === null) tail = iteratorSeq(iterator);
        return tail;
      }; })()]]);
  }
  function first(value) {
    if (value == null) return null;
    const method = implementsProtocol(ISeq, value) ? publicMethod(value, "ISeq/-first", 1) : null;
    if (method) return method.call(value, value);
    if (chunks.get(value)?.integer) return chunks.get(value).start;
    if (lazyValues.has(value)) { seq(value); return first(lazyValues.get(value).value); }
    const current = seq(value);
    if (current === null) return null;
    const cell = cells.get(current);
    if (cell) return cell.head;
    const slice = views.get(current);
    if (slice) return slice.storage[slice.offset];
    const firstMethod = builtin(current, "ISeq/-first", 1);
    if (firstMethod) return firstMethod.call(current, current);
    throw new Error("ISeq.-first not supported on this type");
  }

  function rest(value) {
    if (value == null) return emptyList;
    const method = implementsProtocol(ISeq, value) ? publicMethod(value, "ISeq/-rest", 1) : null;
    if (method) return method.call(value, value);
    if (lazyValues.has(value)) { seq(value); return rest(lazyValues.get(value).value); }
    const current = seq(value);
    if (current === null) return emptyList;
    const cell = cells.get(current);
    if (cell) return cell.tail ?? emptyList;
    const slice = views.get(current);
    if (!slice) {
      const restMethod = builtin(current, "ISeq/-rest", 1);
      if (restMethod) return restMethod.call(current, current);
      throw new Error("ISeq.-rest not supported on this type");
    }
    const tail = slice.offset + 1 < slice.storage.length
      ? view(slice.storage, slice.offset + 1, slice.indexed, slice.kind, slice.tail) : slice.tail ?? emptyList;
    return slice.kind === "queue" ? withMeta(tail, meta(current)) : tail;
  }

  function next(value) {
    if (value == null) return null;
    const method = implementsProtocol(INext, value) ? publicMethod(value, "INext/-next", 1) : null;
    if (method) return method.call(value, value);
    if (lazyValues.has(value)) { seq(value); return next(lazyValues.get(value).value); }
    return seq(rest(value));
  }

  function seqToMap(value) {
    // Upstream first probes next, then realizes exactly this argument sequence.
    // A singleton is its value (including nil); an odd trailing value is conjoined.
    if (next(value) === null) return seq(value) === null ? mapFromArray([]) : first(value);
    const values = toArray(value), trailing = values.length % 2 ? values.pop() : null;
    const base = mapFromArray(values, "array-map");
    return trailing == null ? base : into(base, trailing);
  }

  function reduced(value) {
    const marker = builtinValue("Reduced", reducedPrototype);
    reducedValues.set(marker, value);
    return marker;
  }

  function isReduced(value) { return reducedValues.has(value); }
  function ensureReduced(value) { return isReduced(value) ? value : reduced(value); }
  function unreduced(value) { return isReduced(value) ? reducedValues.get(value) : value; }
  function deref(value) { return derefProtocol(value); }
  function derefProtocol(value) {
    const method = builtin(value, "IDeref/-deref", 1);
    if (method) return method.call(value, value);
    if (delays.has(value)) return deferredValue(delays.get(value));
    if (vars.has(value)) return vars.get(value).get.call(null);
    if (atoms.has(value) || volatiles.has(value)) return value.state;
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

  // Numeric core functions use JavaScript Number/coercion rules. In particular,
  // single-arity CLJS functions accept missing/extra JS arguments, and the
  // variadic dispatcher falls through to its two leading slots at arity zero.
  function numericFold(operation, x, y, more) {
    let value = operation(x, y);
    for (const item of more) value = operation(value, item);
    return value;
  }
  function subtract(x, y, ...more) {
    return arguments.length === 1 ? -x : numericFold((a, b) => a - b, x, y, more);
  }
  function multiply(x, y, ...more) {
    if (arguments.length === 0) return 1;
    return arguments.length === 1 ? x : numericFold((a, b) => a * b, x, y, more);
  }
  function divide(x, y, ...more) {
    return arguments.length === 1 ? 1 / x : numericFold((a, b) => a / b, x, y, more);
  }
  function comparison(test) {
    const fn = function (x, y, ...more) {
      if (arguments.length === 1) return true;
      if (!test(x, y)) return false;
      for (const item of more) { if (!test(y, item)) return false; y = item; }
      return true;
    };
    return variadicCore(fn, 2, (x, y, more) => {
      if (!test(x, y)) return false;
      for (let s = seq(more); s !== null; s = next(s)) {
        const item = first(s);
        if (!test(y, item)) return false;
        y = item;
      }
      return true;
    });
  }
  function extrema(test) {
    return function (x, y, ...more) {
      if (arguments.length === 1) return x;
      return numericFold((a, b) => isNaN(a) ? a : isNaN(b) ? b : test(a, b) ? a : b, x, y, more);
    };
  }
  // parseInt/parseFloat are intentional: CLJS integer? excludes numbers printed
  // in exponent notation even when Number.isInteger would accept them.
  const isInteger = x => typeof x === "number" && !isNaN(x) && x !== Infinity && parseFloat(x) === parseInt(x, 10);
  const fix = x => x >= 0 ? Math.floor(x) : Math.ceil(x);
  const mod = (n, d) => ((n % d) + d) % d;
  const quot = (n, d) => fix((n - n % d) / d);
  function isEven(n) {
    if (!isInteger(n)) throw new Error("Argument must be an integer: " + n);
    return (n & 1) === 0;
  }
  function char(x) {
    if (typeof x === "number") return String.fromCharCode(x);
    if (typeof x === "string" && x.length === 1) return x;
    throw new Error("Argument to char must be a character or number");
  }
  function bitOperation(operation) {
    return variadicCore((x, y, ...more) => numericFold(operation, x, y, more), 2,
      (x, y, more) => foldSeq(operation, x, y, more));
  }
  function bitCount(v) {
    v = v - ((v >> 1) & 0x55555555);
    v = (v & 0x33333333) + ((v >> 2) & 0x33333333);
    return (((v + (v >> 4)) & 0x0f0f0f0f) * 0x01010101) >> 24;
  }
  const numeric = {
    subtract, multiply, divide, inc: x => x + 1, dec: x => x - 1,
    less: comparison((x, y) => x < y), lessEqual: comparison((x, y) => x <= y),
    greater: comparison((x, y) => x > y), greaterEqual: comparison((x, y) => x >= y),
    numericEqual: function (...values) { return values.length !== 0 && equal(...values); },
    max: extrema((x, y) => x > y), min: extrema((x, y) => x < y), abs: x => Math.abs(x),
    int: x => x | 0, long: x => fix(x), uncheckedInt: x => fix(x), uncheckedLong: x => fix(x),
    byte: x => x, short: x => x, float: x => x, double: x => x, char,
    booleans: x => x,
    bytes: x => x,
    chars: x => x,
    shorts: x => x,
    ints: x => x,
    floats: x => x,
    doubles: x => x,
    longs: x => x,
    uncheckedByte: x => x, uncheckedShort: x => x, uncheckedFloat: x => x,
    uncheckedDouble: x => x, uncheckedChar: x => x,
    uncheckedAdd: (...xs) => add(...xs), uncheckedAddInt: (...xs) => add(...xs),
    uncheckedSubtract: (...xs) => subtract(...xs), uncheckedSubtractInt: (...xs) => subtract(...xs),
    uncheckedMultiply: (...xs) => multiply(...xs), uncheckedMultiplyInt: (...xs) => multiply(...xs),
    uncheckedDivideInt: (...xs) => divide(...xs), uncheckedInc: x => x + 1, uncheckedIncInt: x => x + 1,
    uncheckedDec: x => x - 1, uncheckedDecInt: x => x - 1,
    uncheckedNegate: x => -x, uncheckedNegateInt: x => -x, uncheckedRemainderInt: (n, d) => mod(n, d),
    jsMod: (n, d) => n % d, mod, quot, rem: (n, d) => n - d * quot(n, d),
    bitXor: bitOperation((x, y) => x ^ y), bitAnd: bitOperation((x, y) => x & y),
    bitOr: bitOperation((x, y) => x | y), bitAndNot: bitOperation((x, y) => x & ~y),
    bitNot: x => ~x, bitClear: (x, n) => x & ~(1 << n), bitFlip: (x, n) => x ^ (1 << n),
    bitSet: (x, n) => x | (1 << n), bitTest: (x, n) => (x & (1 << n)) !== 0,
    bitShiftLeft: (x, n) => x << n, bitShiftRight: (x, n) => x >> n,
    bitShiftRightZeroFill: (x, n) => x >>> n, unsignedBitShiftRight: (x, n) => x >>> n, bitCount,
    isZero: x => x === 0, isPositive: x => x > 0, isNegative: x => x < 0,
    isEven, isOdd: x => !isEven(x), isInteger, isInt: x => isInteger(x),
    isPositiveInt: x => isInteger(x) && x > 0, isNegativeInt: x => isInteger(x) && x < 0,
    isNaturalInt: x => isInteger(x) && !(x < 0),
    isNumber: x => typeof x === "number", isFloat: x => typeof x === "number", isDouble: x => typeof x === "number",
    isInfinite: x => x === Infinity || x === -Infinity, isNaN: x => isNaN(x)
  };

  const isIdent = x => keywords.has(x) || symbols.has(x);
  const isColl = x => !!directMethod(x, "ICollection", 0) || vectors.has(x) || cells.has(x) || views.has(x) || queues.has(x) || lazyValues.has(x) || maps.has(x) || sets.has(x);
  const predicates = {
    boolean: x => x != null && x !== false, not: x => x == null || x === false,
    isBoolean: x => x === true || x === false, isTrue: x => x === true, isFalse: x => x === false,
    isNil: x => x == null, isSome: x => x != null, isUndefined: x => x === undefined,
    notEqual: (...xs) => !equal(...xs), identity: x => x, isAny: _x => true,
    isString: x => typeof x === "string", isChar: x => typeof x === "string" && x.length === 1,
    isKeyword: x => keywords.has(x), isSymbol: x => symbols.has(x), isIdent,
    isSimpleIdent: x => isIdent(x) && namespace(x) == null,
    isQualifiedIdent: x => isIdent(x) && namespace(x) != null,
    isSimpleKeyword: x => keywords.has(x) && namespace(x) == null,
    isQualifiedKeyword: x => keywords.has(x) && namespace(x) != null,
    isSimpleSymbol: x => symbols.has(x) && namespace(x) == null,
    isQualifiedSymbol: x => symbols.has(x) && namespace(x) != null,
    keywordIdentical: (x, y) => x === y || (keywords.has(x) && keywords.has(y) && equalPair(x, y)),
    symbolIdentical: (x, y) => x === y || (symbols.has(x) && symbols.has(y) && equalPair(x, y)),
    isFn: x => typeof x === "function" || satisfies(Fn, x),
    isIFn: x => predicates.isFn(x) || satisfies(IFn, x),
    isArray: x => Array.isArray(x),
    isObject: x => x != null && !isColl(x) && !isIdent(x) && !uuids.has(x) && !vars.has(x) && !reducedValues.has(x) && x.constructor === Object,
    isColl: x => x != null && satisfies(ICollection, x), isSeq: x => x != null && satisfies(ISeq, x), isList: x => satisfies(IList, x),
    isSeqable: x => x == null || satisfies(ISeqable, x) || Array.isArray(x) || typeof x === "string" || x[Symbol.iterator] != null,
    isSequential: x => satisfies(ISequential, x), isVector: x => satisfies(IVector, x), isMap: x => x != null && satisfies(IMap, x), isSet: x => x != null && satisfies(ISet, x),
    isAssociative: x => satisfies(IAssociative, x), isIFind: x => satisfies(IFind, x),
    isCounted: x => satisfies(ICounted, x),
    isIndexed: x => satisfies(IIndexed, x),
    isReduceable: x => satisfies(IReduce, x),
    isReversible: x => satisfies(IReversible, x),
    isSorted: x => satisfies(ISorted, x), isRecord: x => satisfies(IRecord, x), isMapEntry: x => implementsProtocol(IMapEntry, x),
    isChunkedSeq: x => implementsProtocol(IChunkedSeq, x),
    isEmpty: x => x == null || (satisfies(ICounted, x) ? core.countProtocol(x) === 0 : seq(x) === null)
  };

  function reduce(rf, init, collection) {
    const arity = arguments.length;
    if (arity !== 2 && arity !== 3) return invalidArity(arity);
    const source = arity === 3 ? collection : init;
    const method = publicMethod(source, "IReduce/-reduce", arity, true);
    if (method) return arity === 3 ? method.call(source, source, rf, init) : method.call(source, source, rf);
    if (!builtinCoreImplementation(IReduce, source) && !Array.isArray(source) && typeof source !== "string" && satisfies(IIterable, source)) {
      const iter = core.iteratorProtocol(source);
      if (arity === 2) { if (!iter.hasNext()) return rf.call(null); init = iter.next(); }
      while (iter.hasNext()) {
        init = rf.call(null, init, iter.next());
        if (isReduced(init)) return deref(init);
      }
      return init;
    }
    const producer = cells.get(source);
    if (producer?.kind === "iterate") {
      let value = first(source);
      if (arity === 2) { init = value; value = producer.fn.call(null, value); }
      while (true) { init = rf.call(null, init, value); if (isReduced(init)) return deref(init); value = producer.fn.call(null, value); }
    }
    if (producer?.kind === "cycle") {
      let current = producer.currentValue();
      if (arity === 2) { init = first(current); current = next(current) ?? producer.all; }
      while (true) { init = rf.call(null, init, first(current)); if (isReduced(init)) return deref(init); current = next(current) ?? producer.all; }
    }
    if (chunks.has(source) && !chunks.get(source).integer) {
      const c = chunks.get(source); let i = c.offset;
      if (arity === 2) init = c.array[i++];
      for (; i < c.array.length; i++) { init = rf.call(null, init, c.array[i]); if (isReduced(init)) return deref(init); }
      return init;
    }
    const sequenceReduction = cells.has(source) || lazyValues.has(source) || (!builtinCoreImplementation(IReduce, source) && !Array.isArray(source) && typeof source !== "string");
    if (arity === 2 && sequenceReduction) {
      const current = seq(source);
      return current == null ? rf.call(null) : reduce(rf, first(current), next(current));
    }
    if (sequenceReduction) {
      for (let current = seq(source); current != null; current = next(current)) {
        init = rf.call(null, init, first(current));
        if (isReduced(init)) return deref(init);
      }
      return init;
    }
    let initialized = arity === 3, result = init;
    let current = seq(initialized ? collection : init);
    let slice = views.get(current), offset = slice?.offset;
    // Traverse immutable storage directly. No recursive calls, per-step seq
    // wrappers, captured closures or persistent traversal caches are needed.
    while (current !== null) {
      const cell = cells.get(current);
      const item = cell ? cell.head : slice ? slice.storage[offset] : first(current);
      if (!initialized) {
        result = item;
        initialized = true;
      } else {
        // Read .call on every invocation, after the source/arguments have been
        // evaluated. Callable data and user functions share this ordinary ABI.
        result = rf.call(null, result, item);
        if (isReduced(result)) return deref(result);
      }
      if (cell || !slice) {
        current = cell ? seq(cell.tail) : next(current);
        slice = views.get(current);
        offset = slice?.offset;
      } else if (++offset === slice.storage.length) {
        current = seq(slice.tail);
        slice = views.get(current); offset = slice?.offset;
      }
    }
    return initialized ? result : rf.call(null);
  }

  // Reducers and transforms use the same .call ABI as emitted functions and
  // custom IFn values. State is allocated when a transform receives its reducer.
  function reducingFunction(rf, step, complete = result => rf.call(null, result)) {
    return function (result, input) {
      switch (arguments.length) {
        case 0: return rf.call(null);
        case 1: return complete(result);
        case 2: return step(result, input);
        default: return invalidArity(arguments.length);
      }
    };
  }
  function transform(factory) {
    return function (rf) {
      return factory(rf);
    };
  }
  function completing(rf, cf) {
    if (arguments.length === 1) cf = result => result;
    else if (arguments.length !== 2) return invalidArity(arguments.length);
    return reducingFunction(rf, (result, input) => rf.call(null, result, input), result => cf.call(null, result));
  }
  function transduce(xf, rf, init, coll) {
    if (arguments.length === 3) { coll = init; init = rf.call(null); }
    else if (arguments.length !== 4) return invalidArity(arguments.length);
    const transformed = xf.call(null, rf);
    return transformed.call(null, reduce(transformed, init, coll));
  }
  function indexedTransform(fn, keepOnly, indexed) {
    return transform(rf => {
      let index = -1;
      return reducingFunction(rf, (result, input) => {
        const value = indexed ? fn.call(null, ++index, input) : fn.call(null, input);
        return keepOnly && value == null ? result : rf.call(null, result, value);
      });
    });
  }
  function bufferedTransform(group) {
    return transform(rf => {
      const none = keyword("cljs.core", "none");
      let buffer = [], prior = none;
      const flush = result => {
        const value = vectorFromArray(buffer); buffer = [];
        return rf.call(null, result, value);
      };
      return reducingFunction(rf, (result, input) => {
        if (group.kind === "count") {
          buffer.push(input);
          return buffer.length === group.n ? flush(result) : result;
        }
        const previous = prior, key = group.fn.call(null, input); prior = key;
        if (predicates.keywordIdentical(previous, none) || equalPair(previous, key)) { buffer.push(input); return result; }
        const ret = flush(result);
        if (!isReduced(ret)) buffer.push(input);
        return ret;
      }, result => {
        // Clear before invoking the downstream step, including reentry/failure.
        if (buffer.length) result = unreduced(flush(result));
        return rf.call(null, result);
      });
    });
  }
  function haltWhen(pred, retf) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    const halt = keyword("cljs.core", "halt");
    return transform(rf => reducingFunction(rf, (result, input) =>
      truth(pred.call(null, input))
        ? reduced(hashMap(halt, truth(retf) ? retf.call(null, rf.call(null, result), input) : input))
        : rf.call(null, result, input),
      result => predicates.isMap(result) && contains(result, halt) ? get(result, halt) : rf.call(null, result)));
  }
  function dedupe(coll) {
    if (arguments.length === 0) return transform(rf => {
      let prior = keyword("cljs.core", "none");
      return reducingFunction(rf, (result, input) => {
        const previous = prior; prior = input;
        return equalPair(previous, input) ? result : rf.call(null, result, input);
      });
    });
    if (arguments.length !== 1) return invalidArity(arguments.length);
    // Collection overload stays lazy; sequence's transducer overload is separate.
    const step = (values, prior) => lazySequence(() => {
      for (let s = seq(values); s !== null; s = seq(rest(s))) {
        const value = first(s);
        if (!equalPair(prior, value)) return cons(value, step(rest(s), value));
      }
      return null;
    });
    return step(coll, keyword("cljs.core", "none"));
  }
  function randomSample(prob, coll) {
    const pred = () => core.rand.call(null) < prob;
    if (arguments.length === 1) return filter(pred);
    if (arguments.length === 2) return filter(pred, coll);
    return invalidArity(arguments.length);
  }

  function reduceKv(rf, init, collection) {
    const method = publicMethod(collection, "IKVReduce/-kv-reduce", 3);
    if (method) return method.call(collection, collection, rf, init);
    if (collection == null) return init;
    const entries = maps.get(collection)?.map(entry => [entry.key, entry.value]) ?? vectors.get(collection)?.map((value, key) => [key, value]);
    if (!entries) return reduce((result, entry) => rf.call(null, result, core.keyProtocol(entry), core.valProtocol(entry)), init, collection);
    for (const [key, value] of entries) {
      init = rf.call(null, init, key, value);
      if (isReduced(init)) return deref(init);
    }
    return init;
  }
  const subvecSources = new WeakMap();
  function subvec(value, start, end = count(value)) {
    if (start == null || end == null) throw new Error("Assert failed: subvec indices cannot be nil");
    start = start | 0; end = end | 0;
    if (start < 0 || end < start || end > count(value)) throw new Error("Index out of bounds");
    if (!predicates.isVector(value)) throw new Error("subvec requires a vector");
    const result = vectorFromArray(Array.from({length: Math.ceil(end - start)}, (_, i) => nth(value, start + i)));
    Object.setPrototypeOf(result, builtinType("Subvec", vectorPrototype).prototype);
    subvecSources.set(result, subvecSources.get(value) ?? value);
    return result;
  }
  class SeqIter {
    constructor(value) { this.value = value; this.started = false; this.consumed = false; }
    hasNext() {
      if (!this.started) { this.value = seq(this.value); this.started = true; }
      else if (this.consumed) this.value = next(this.value);
      this.consumed = false;
      return this.value !== null;
    }
    next() {
      if (!this.hasNext()) throw new Error("No such element");
      this.consumed = true;
      return first(this.value);
    }
    remove() { return new Error("Unsupported operation"); }
  }
  class RangedIterator extends SeqIter {}
  function iterator(value) {
    const method = builtin(value, "IIterable/-iterator", 1);
    if (method) return method.call(value, value);
    const source = subvecSources.get(value) ?? value;
    return vectors.has(source) ? new RangedIterator(value) : new SeqIter(value);
  }
  function equivMap(value, other) {
    if (!predicates.isMap(other)) return false;
    let n = 0;
    for (let current = seq(value); current != null; current = next(current), n++) {
      const entry = first(current), found = get(other, nth(entry, 0), missing);
      if (found === missing || !equalPair(found, nth(entry, 1))) return false;
    }
    return n === count(other);
  }

  function conj(...args) {
    if (args.length === 0) return vector();
    let result = args[0];
    for (let i = 1; i < args.length; i++) {
      const previous = result;
      const custom = result == null ? null : builtin(result, "ICollection/-conj", 2);
      if (custom) { result = custom.call(result, result, args[i]); continue; }
      const items = vectors.get(result);
      if (queues.has(result)) result = queueFromArray([...queues.get(result), args[i]]);
      else if (items) result = makeVector([...items, args[i]]);
      else if (maps.has(result)) result = mapConj(result, args[i]);
      else if (sets.has(result)) {
        const entries = sets.get(result).slice();
        putCollectionEntry(result, entries, args[i], undefined, true);
        if (entries.length === sets.get(result).length) continue;
        result = sortedComparators.has(previous) ? makeSet(entries, "sorted-set", sortedComparators.get(previous)) : makeSet(entries, setKinds.get(previous) === "hash-map" || entries.length > 8 ? "hash-map" : "array-map");
      }
      else if (result == null) result = cons(args[i], emptyList, "list");
      else if (cells.has(result) || views.has(result) || lazyValues.has(result))
        result = cons(args[i], result, cells.get(result)?.kind === "list" ? "list" : "cons");
      else throw new Error("conj not supported on this type");
      if (vectors.has(previous) || maps.has(previous) || sets.has(previous) || queues.has(previous) || cells.get(previous)?.kind === "list") result = preserveMeta(previous, result);
    }
    return result;
  }

  function assoc(value, key, item, ...pairs) {
    const method = value == null ? null : builtin(value, "IAssociative/-assoc", 3);
    if (method) {
      let result = method.call(value, value, key, item);
      return pairs.length ? assoc(result, ...pairs) : result;
    }
    if (value == null || maps.has(value)) {
      const entries = value == null ? [] : maps.get(value).slice();
      putCollectionEntry(value, entries, key, item);
      for (let i = 0; i < pairs.length; i += 2)
        putCollectionEntry(value, entries, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
      return updatedMap(value, entries);
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
    return preserveMeta(value, makeVector(items));
  }

  const mapPrototype = Object.freeze({
    has(key) { return contains(this, key); },
    get(key, notFound = null) { return get(this, key, notFound); },
    keys() { return es6Iterator(keys(this)); },
    values() { return es6Iterator(vals(this)); },
    entries() { return es6EntriesIterator(this); },
    [Symbol.iterator]() { return es6Iterator(this); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return print(this, true); },
    call(_self, key, notFound) {
      if (!maps.has(this)) throw new Error("Expected map");
      switch (arguments.length) {
        case 2: return lookup(this, key, null);
        case 3: return lookup(this, key, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    }
  });

  function makeMap(entries, kind = entries.length <= 8 ? "array-map" : "hash-map", comparator = compareKeys) {
    const value = builtinValue(kind === "hash-map" ? "PersistentHashMap" : kind === "sorted-map" ? "PersistentTreeMap" : "PersistentArrayMap", mapPrototype);
    maps.set(value, Object.freeze(entries));
    mapKinds.set(value, kind);
    if (kind === "sorted-map") sortedComparators.set(value, comparator);
    return value;
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

  function mapFromArray(pairs, kind) {
    if (pairs.length % 2) throw new Error("No value supplied for key: " + str(pairs[pairs.length - 1]));
    const entries = [];
    for (let i = 0; i < pairs.length; i += 2) putEntry(entries, pairs[i], pairs[i + 1]);
    return makeMap(entries, kind);
  }

  function hashMap(...pairs) { return mapFromArray(pairs, "hash-map"); }
  function arrayMap(...pairs) { return mapFromArray(pairs, "array-map"); }

  function dissoc(value, ...keys) {
    if (value != null && builtin(value, "IMap/-dissoc", 2)) return keys.reduce((result, key) => {
      const method = result == null ? null : builtin(result, "IMap/-dissoc", 2);
      return method ? method.call(result, result, key) : dissoc(result, key);
    }, value);
    if (arguments.length === 0) return null;
    if (keys.length === 0 || value == null) return value;
    const original = maps.get(value);
    if (!original) throw new Error("dissoc not supported on this type");
    const entries = original.slice();
    for (const key of keys) {
      const index = collectionEntryIndex(value, entries, key);
      if (index >= 0) entries.splice(index, 1);
    }
    const Type = recordTypes.get(value);
    const removeRecord = Type && keys.some(key => Type.fieldNames.some(name => equalPair(key, keyword(null, name))));
    return updatedMap(value, entries, removeRecord);
  }

  function mapConj(value, member) {
    if (member == null) return value;
    if (predicates.isVector(member)) return assoc(value, core.nthProtocol(member, 0), core.nthProtocol(member, 1));
    const entries = maps.get(value).slice();
    for (let current = seq(member); current !== null; current = next(current)) {
      const pair = first(current);
      if (!predicates.isVector(pair)) throw new Error("Map conj requires vector entries");
      putCollectionEntry(value, entries, core.nthProtocol(pair, 0), core.nthProtocol(pair, 1));
    }
    return updatedMap(value, entries);
  }

  const setPrototype = Object.freeze({
    has(key) { return contains(this, key); },
    keys() { return es6Iterator(this); },
    values() { return es6Iterator(this); },
    entries() { return es6EntriesIterator(this, true); },
    [Symbol.iterator]() { return es6Iterator(this); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return print(this, true); },
    call(_self, key, notFound) {
      if (!sets.has(this)) throw new Error("Expected set");
      switch (arguments.length) {
        case 2: return lookup(this, key, null);
        case 3: return lookup(this, key, notFound);
        default: return invalidArity(arguments.length - 1);
      }
    }
  });

  function makeSet(entries, kind = entries.length > 8 ? "hash-map" : "array-map", comparator = compareKeys) {
    const value = builtinValue(kind === "sorted-set" ? "PersistentTreeSet" : "PersistentHashSet", setPrototype);
    sets.set(value, Object.freeze(entries));
    setKinds.set(value, kind);
    if (kind === "sorted-set") sortedComparators.set(value, comparator);
    return value;
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

  function literalSetFromArray(items) {
    const entries = [];
    for (const item of items) {
      const before = entries.length;
      putMember(entries, item);
      if (entries.length === before) throw new Error("Duplicate key: " + str(item));
    }
    return makeSet(entries);
  }

  function hashSet(...items) { return setFromArray(items); }

  function disj(value, ...keys) {
    if (value != null && builtin(value, "ISet/-disjoin", 2)) return keys.reduce((result, key) => builtin(result, "ISet/-disjoin", 2).call(result, result, key), value);
    if (arguments.length === 0) return null;
    if (keys.length === 0) return value;
    if (value == null) return null;
    const original = sets.get(value);
    if (!original) throw new Error("disj not supported on this type");
    const entries = original.slice();
    for (const key of keys) {
      const index = collectionEntryIndex(value, entries, key);
      if (index >= 0) entries.splice(index, 1);
    }
    if (entries.length === original.length) return value;
    return preserveMeta(value, makeSet(entries, setKinds.get(value), sortedComparators.get(value)));
  }

  // Transient payloads own a private mutable copy of canonical collection
  // storage. Persistence freezes that storage; no persistent alias can mutate it.
  const transientPrototype = Object.freeze({
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return print(this, true); },
    call(_self, key, notFound) {
      if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length - 1);
      return transientLookup(this, key, arguments.length === 2 ? null : notFound);
    }
  });
  function makeTransient(kind, entries, mapKind) {
    const name = kind === "vector" ? "TransientVector" : kind === "set" ? "TransientHashSet" : mapKind === "array-map" ? "TransientArrayMap" : "TransientHashMap";
    const result = builtinValue(name, transientPrototype);
    transients.set(result, {kind, entries, mapKind, editable: true});
    return result;
  }
  function editableTransient(value, operation) {
    const state = transients.get(value);
    if (!state?.editable) throw new Error(operation === "persistent!" ? "persistent! called twice" : operation + " after persistent!");
    return state;
  }
  function transient(value) { return core.asTransientProtocol(value); }
  function asTransient(value) {
    if (vectors.has(value)) return makeTransient("vector", vectors.get(value).slice());
    if (maps.has(value)) return makeTransient("map", maps.get(value).slice(), mapKinds.get(value));
    return makeTransient("set", sets.get(value).slice(), setKinds.get(value));
  }
  function persistentBang(value) { return core.persistentBangProtocol(value); }
  function persistTransient(value) {
    const state = editableTransient(value, "persistent!");
    state.editable = false;
    if (state.kind === "vector") return makeVector(state.entries);
    if (state.kind === "map") return makeMap(state.entries, state.mapKind);
    return makeSet(state.entries, state.mapKind);
  }
  function transientNth(value, index, fallback) {
    const state = transients.get(value);
    // Pinned CLJS's three-argument -nth returns an out-of-range fallback
    // even after persistence; in-range access still checks ownership.
    if (fallback !== missing && !(index >= 0 && index < state.entries.length)) return fallback;
    editableTransient(value, "nth");
    return vectorNth(state.entries, index, fallback);
  }
  function transientLookup(value, key, notFound) {
    const state = transients.get(value);
    // TransientHashMap (and its set wrapper) retains readable lookup after
    // persistence in both pinned versions. Array maps and vectors reject it.
    if (state.mapKind !== "hash-map") editableTransient(value, "lookup");
    if (state.kind === "vector") return typeof key === "number" ? transientNth(value, key, notFound) : notFound;
    const index = entryIndex(state.entries, key, hash(key));
    return index < 0 ? notFound : state.kind === "set" ? key : state.entries[index].value;
  }
  function assocTransient(value, key, item, indexed = false) {
    const state = transients.get(value);
    if (state.kind === "vector") {
      if (!indexed && typeof key !== "number") throw new Error("TransientVector's key for assoc! must be a number.");
      editableTransient(value, "assoc!");
      if (key >= 0 && key < state.entries.length) state.entries[key & 0x7fffffff] = item;
      else if (key === state.entries.length) state.entries.push(item);
      else outOfBounds();
    } else {
      editableTransient(value, "assoc!");
      if (state.mapKind === "array-map" && state.entries.length >= 8 && entryIndex(state.entries, key, hash(key)) < 0)
        return assocTransient(makeTransient("map", state.entries.slice(), "hash-map"), key, item);
      putEntry(state.entries, key, item);
    }
    return value;
  }
  function conjTransient(value, member) {
    const state = editableTransient(value, "conj!");
    if (state.kind === "vector") { state.entries.push(member); return value; }
    if (state.kind === "set") {
      putMember(state.entries, member);
      if (state.entries.length > 8) state.mapKind = "hash-map";
      return value;
    }
    if (predicates.isMapEntry(member)) return core.assocBangProtocol(value, core.keyProtocol(member), core.valProtocol(member));
    if (predicates.isVector(member)) return core.assocBangProtocol(value, member.call(null, 0), member.call(null, 1));
    for (let s = seq(member); s !== null && truth(first(s)); s = next(s)) {
      const entry = first(s);
      value = core.assocBangProtocol(value, core.keyProtocol(entry), core.valProtocol(entry));
    }
    return value;
  }
  function removeTransient(value, key, operation) {
    const state = editableTransient(value, operation), index = entryIndex(state.entries, key, hash(key));
    if (index >= 0) {
      if (state.mapKind === "array-map") state.entries[index] = state.entries[state.entries.length - 1];
      state.entries.splice(state.mapKind === "array-map" ? state.entries.length - 1 : index, 1);
    }
    return value;
  }
  function popTransient(value) {
    const state = editableTransient(value, "pop!");
    if (!state.entries.length) throw new Error("Can't pop empty vector");
    state.entries.pop(); return value;
  }
  function conjBang(value, ...members) {
    if (!arguments.length) return transient(vector());
    for (const member of members) value = core.conjBangProtocol(value, member);
    return value;
  }
  function assocBang(value, key, item, ...pairs) {
    value = core.assocBangProtocol(value, key, item);
    for (let i = 0; i < pairs.length; i += 2) value = core.assocBangProtocol(value, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
    return value;
  }
  function dissocBang(value, key, ...keys) {
    value = core.dissocBangProtocol(value, key);
    for (const key of keys) value = core.dissocBangProtocol(value, key);
    return value;
  }
  function disjBang(value, key, ...keys) {
    value = core.disjoinBangProtocol(value, key);
    for (const key of keys) value = core.disjoinBangProtocol(value, key);
    return value;
  }
  function popBang(value) { return core.popBangProtocol(value); }

  const vectorPrototype = Object.freeze({
    [Symbol.iterator]() { return es6Iterator(this); },
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return print(this, true); },
    indexOf(value, start = 0) { return sequentialIndex(this, value, start, false); },
    lastIndexOf(value, start = count(this)) { return sequentialIndex(this, value, start, true); },
    // The pinned fixed method accepts one index and ignores extra arguments.
    call(_self, index) {
      const items = vectors.get(this);
      if (!items) throw new Error("Expected vector");
      if (mapEntries.has(this)) {
        if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length - 1);
        if (index === 0 || index === 1) return items[index];
        return arguments.length === 3 ? arguments[2] : outOfBounds();
      }
      if (typeof index !== "number") throw new Error("Key must be integer");
      return vectorNth(items, index, missing);
    }
  });

  function makeKeyword(namespace, name, spelling) {
    const value = builtinValue("Keyword", keywordPrototype);
    keywords.set(value, Object.freeze({namespace, name, spelling}));
    return value;
  }

  // Literal construction receives separately owned namespace/name strings.
  function keyword(namespace, name) {
    return makeKeyword(namespace, name,
      namespace == null ? name : namespace + "/" + name);
  }

  function makeVector(items) {
    const value = builtinValue("PersistentVector", vectorPrototype);
    vectors.set(value, Object.freeze(items));
    return value;
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
    if (multiFns.has(value)) return name(value.name);
    const method = implementsProtocol(INamed, value) ? builtin(value, "INamed/-name", 1) : null;
    if (method) return method.call(value, value);
    const named = keywords.get(value) || symbols.get(value);
    if (named) return named.name;
    if (typeof value === "string") return value;
    throw new Error("Doesn't support name on this type");
  }

  // ClojureScript subs delegates index coercion and bounds to JS substring.
  function subs(value, start, end) {
    switch (arguments.length) {
      case 2: return value.substring(start);
      case 3: return value.substring(start, end);
      default: return invalidArity(arguments.length);
    }
  }

  function namespace(value) {
    if (multiFns.has(value)) return namespace(value.name);
    const method = implementsProtocol(INamed, value) ? builtin(value, "INamed/-namespace", 1) : null;
    if (method) return method.call(value, value);
    const named = keywords.get(value) || symbols.get(value);
    if (named) return named.namespace;
    throw new Error("Doesn't support namespace on this type");
  }

  function count(value) {
    const method = value == null ? null : publicMethod(value, "ICounted/-count", 1, true, true);
    if (method) return method.call(value, value);
    if (transients.has(value)) return editableTransient(value, "count").entries.length;
    if (lazyValues.has(value)) return accumulatingCount(value);
    if (chunkBuffers.has(value)) return chunkBuffers.get(value).end;
    if (chunks.has(value)) { const c = chunks.get(value); return c.end - c.offset; }
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
    if (cell) return "count" in cell ? cell.count : accumulatingCount(value);
    const slice = views.get(value);
    if (slice) return Math.max(0, slice.storage.length - slice.offset) + count(slice.tail);
    if (implementsProtocol(ISeqable, value)) return accumulatingCount(value);
    const nativeCount = builtin(value, "ICounted/-count", 1);
    if (nativeCount) return nativeCount.call(value, value);
    throw new Error("count not supported on this type");
  }

  function accumulatingCount(value) {
    let result = 0;
    for (let current = seq(value); current != null; current = next(current), result++) {
      if (satisfies(ICounted, current)) return result + core.countProtocol(current);
    }
    return result;
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
    const method = publicMethod(value, "IIndexed/-nth", arity, true, true);
    if (method) return method.call(value, ...arguments);
    if (transients.has(value)) return transientNth(value, index, fallback);
    if (chunks.has(value)) return chunkNth(value, index, fallback);
    if (cells.get(value)?.integer) { const c = cells.get(value); return index >= 0 && index < c.count ? c.head + Math.floor(index) * c.step : fallback === missing ? outOfBounds() : fallback; }
    if (Array.isArray(value)) return index > -1 && index < value.length ? value[Math.trunc(index)] : fallback === missing ? outOfBounds() : fallback;
    const items = vectors.get(value);
    if (items) return mapEntries.has(value) && index !== 0 && index !== 1 ? fallback === missing ? outOfBounds() : fallback : vectorNth(items, index, fallback);
    if (typeof value === "string") {
      if (index > -1 && index < value.length) return value.charAt(index | 0);
      if (fallback === missing) return outOfBounds();
      return fallback;
    }
    if (implementsProtocol(ISeq, value) || implementsProtocol(ISequential, value)) {
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
        if (remaining === 0) {
          if (seq(current) != null) return first(current);
          break;
        }
        if (satisfies(IIndexed, current)) return fallback === missing ? core.nthProtocol(current, remaining) : core.nthProtocol(current, remaining, fallback);
        if (seq(current) == null) break;
        current = next(current);
        remaining--;
      }
      return fallback === missing ? outOfBounds() : fallback;
    }
    const nativeNth = publicMethod(value, "IIndexed/-nth", arity);
    if (nativeNth) return nativeNth.call(value, ...arguments);
    throw new Error("nth not supported on this type");
  }

  function lookup(value, key, notFound) {
    if (transients.has(value)) return transientLookup(value, key, notFound);
    const members = sets.get(value);
    if (members) {
      const index = collectionEntryIndex(value, members, key);
      return index < 0 ? notFound : members[index].key;
    }
    const entries = maps.get(value);
    if (entries) {
      const index = collectionEntryIndex(value, entries, key);
      return index < 0 ? notFound : entries[index].value;
    }
    const items = vectors.get(value);
    if (items) return mapEntries.has(value) ? key === 0 || key === 1 ? items[key] : notFound : typeof key === "number" && key >= 0 && key < items.length ? items[Math.trunc(key)] : notFound;
    if (typeof value === "string" && key != null && key > -1 && key < value.length) {
      return value.charAt(key | 0);
    }
    // Raw object properties are not lookup entries. Public get handles array
    // indexing separately without giving host objects owned collection storage.
    return notFound;
  }

  function get(value, key, notFound) {
    const method = value == null ? null : publicMethod(value, "ILookup/-lookup", arguments.length, true);
    if (method) return method.call(value, ...arguments);
    if (Array.isArray(value)) {
      const valid = key != null && key < value.length && (arguments.length === 2 || key > -1);
      return valid ? value[key | 0] : arguments.length === 2 ? null : notFound;
    }
    switch (arguments.length) {
      case 2: return lookup(value, key, null);
      case 3: return lookup(value, key, notFound);
      default: return invalidArity(arguments.length);
    }
  }

  function contains(value, key) {
    const method = publicMethod(value, "IAssociative/-contains-key?", 2);
    if (method) return method.call(value, value, key);
    const items = vectors.get(value);
    if (items) return Number.isInteger(key) && key >= 0 && key < items.length;
    return get(value, key, missing) !== missing;
  }

  function every(predicate, value) {
    for (let current = seq(value); current !== null; current = next(current)) if (!truth(predicate.call(null, first(current)))) return false;
    return true;
  }
  function find(value, key) {
    const method = publicMethod(value, "IFind/-find", 2);
    if (method) return method.call(value, value, key);
    if (value == null || !predicates.isAssociative(value) || !contains(value, key)) return null;
    const entries = !recordTypes.has(value) && maps.get(value);
    const entry = entries && entries[collectionEntryIndex(value, entries, key)];
    return mapEntry(entry ? entry.key : key, get(value, key));
  }

  function getIn(value, keys, notFound) {
    if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length);
    const fallback = arguments.length === 3;
    for (let path = seq(keys); path !== null; path = next(path)) {
      value = fallback ? get(value, first(path), missing) : get(value, first(path));
      if (value === missing) return notFound;
    }
    return value;
  }
  function assocIn(value, path, item) {
    const current = seq(path), key = first(current), more = next(current);
    return assoc(value, key, more == null ? item : assocIn(get(value, key), more, item));
  }
  function updateIn(value, path, fn, ...args) {
    const variadic = arguments.length < 3 || arguments.length > 6;
    return updatePath(value, path, old => variadic
      ? apply(fn, old, args[0], args[1], args[2], fnRest(args, 3))
      : fn.call(null, old, ...args));
  }
  function updatePath(value, path, updateLeaf) {
    const current = seq(path), key = first(current), more = next(current);
    const old = get(value, key);
    return assoc(value, key, more == null ? updateLeaf(old) : updatePath(old, more, updateLeaf));
  }
  function selectKeys(value, keyseq) {
    const entries = [];
    for (let current = seq(keyseq); current != null; current = next(current)) {
      const key = first(current), item = get(value, key, missing);
      if (item !== missing) putEntry(entries, key, item);
    }
    return withMeta(makeMap(entries), meta(value));
  }
  function mergeMaps(values, combine) {
    // `some identity` precedes reduction, including its sequence effects.
    let present = false;
    for (let current = seq(values); current != null; current = next(current)) {
      if (truth(first(current))) { present = true; break; }
    }
    if (!present) return null;
    return reduce((left, right) => {
      const target = truth(left) ? left : makeMap([]);
      if (combine === missing) return conj(target, right);
      return reduce((result, entry) => {
        const key = core.keyProtocol(entry), item = core.valProtocol(entry);
        return assoc(result, key, truth(contains(result, key)) ? combine.call(null, get(result, key), item) : item);
      }, target, seq(right));
    }, values);
  }
  function someVals(value) {
    return reduceKv((result, key, item) => item == null ? result : assoc(result, key, item), null, value);
  }
  function requiredGet(value, key) {
    const result = get(value, key, missing);
    if (result === missing) throw new Error("Missing required key: " + str(key));
    return result;
  }
  function keyExtreme(less, key, x, y, more) {
    let result = (less ? key.call(null, x) < key.call(null, y) : key.call(null, x) > key.call(null, y)) ? x : y;
    for (let s = seq(more); s !== null; s = next(s)) {
      const item = first(s);
      result = (less ? key.call(null, result) < key.call(null, item) : key.call(null, result) > key.call(null, item)) ? result : item;
    }
    return result;
  }
  function minKey(key, x, y, ...more) { return arguments.length === 2 ? x : keyExtreme(true, key, x, y, fnRest(more, 0)); }
  function maxKey(key, x, y, ...more) { return arguments.length === 2 ? x : keyExtreme(false, key, x, y, fnRest(more, 0)); }
  function reductions(fn, initial, coll) {
    if (arguments.length === 2) return lazySequence(() => {
      const s = seq(initial);
      return s === null ? list(fn.call(null)) : reductions(fn, first(s), rest(s));
    });
    if (arguments.length !== 3) return invalidArity(arguments.length);
    if (reducedValues.has(initial)) return list(reducedValues.get(initial));
    return cons(initial, lazySequence(() => {
      const s = seq(coll);
      return s === null ? null : reductions(fn, fn.call(null, initial, first(s)), rest(s));
    }));
  }
  function replace(substitutions, coll) {
    const replaceOne = value => { const entry = find(substitutions, value); return entry == null ? value : nth(entry, 1); };
    if (arguments.length === 1) return mapSeq(replaceOne);
    collectionArity("replace", arguments.length);
    if (predicates.isVector(coll)) {
      let result = coll;
      for (let i = 0, size = count(coll); i < size; i++) {
        const entry = find(substitutions, nth(result, i));
        if (entry !== null) result = assoc(result, i, nth(entry, 1));
      }
      return result;
    }
    return mapSeq(replaceOne, coll);
  }
  function distinct(coll) {
    if (arguments.length === 0) return transform(rf => {
      let seen = makeSet([]);
      return reducingFunction(rf, (result, input) => {
        if (contains(seen, input)) return result;
        seen = conj(seen, input);
        return rf.call(null, result, input);
      });
    });
    if (arguments.length !== 1) return invalidArity(arguments.length);
    const step = (values, seen) => lazySequence(() => {
      for (let s = seq(values); s !== null; s = seq(rest(s))) {
        const value = first(s);
        if (!contains(seen, value)) return cons(value, step(rest(s), conj(seen, value)));
      }
      return null;
    });
    return step(coll, makeSet([]));
  }
  function distinctMore(x, y, more) {
    if (equalPair(x, y)) return false;
    let seen = hashSet(x, y), s = more;
    for (;;) {
      const value = first(s), following = next(s);
      if (s == null) return true;
      if (contains(seen, value)) return false;
      seen = conj(seen, value); s = following;
    }
  }
  function isDistinct(x, y, ...more) {
    return arguments.length === 1 ? true : distinctMore(x, y, fnRest(more, 0));
  }
  function fnComparator(fn) {
    if (fn === compareKeys) return fn;
    return (x, y) => {
      const result = fn.call(null, x, y);
      return typeof result === "number" ? result : truth(result) ? -1 : truth(fn.call(null, y, x)) ? 1 : 0;
    };
  }
  function sort(fn, coll) {
    if (arguments.length === 1) { coll = fn; fn = compareKeys; }
    else if (arguments.length !== 2) return invalidArity(arguments.length);
    if (seq(coll) === null) return emptyList;
    const values = toArray(coll);
    values.sort(fnComparator(fn));
    return withMeta(seq(values), meta(coll));
  }
  function sortBy(key, fn, coll) {
    if (arguments.length === 2) { coll = fn; fn = compareKeys; }
    else if (arguments.length !== 3) return invalidArity(arguments.length);
    return sort((x, y) => fnComparator(fn)(key.call(null, x), key.call(null, y)), coll);
  }
  function comparator(predicate) {
    return (x, y) => truth(predicate.call(null, x, y)) ? -1 : truth(predicate.call(null, y, x)) ? 1 : 0;
  }
  function rand(n) {
    if (arguments.length === 0) return Math.random();
    if (arguments.length === 1) return Math.random() * n;
    return invalidArity(arguments.length);
  }
  // Public core utilities follow the pinned core.cljs Number/UTF-16 contracts.
  function randInt(n) { return Math.floor(Math.random() * n); }
  function randNth(coll) { return nth(coll, randInt(count(coll))); }
  function isJsSymbol(value) {
    return typeof value === "symbol" || (typeof Symbol !== "undefined" && value instanceof Symbol);
  }
  const specialSymbols = new Set("if def fn* do let* loop* letfn* throw try catch finally recur new set! ns deftype* defrecord* . js* & quote case* var ns*".split(" "));
  function isSpecialSymbol(value) {
    const data = symbols.get(value);
    return !!data && data.namespace === null && specialSymbols.has(data.name);
  }
  function parsingString(value) {
    if (typeof value !== "string") throw new Error("Expected string, got: " + (value == null ? "nil" : Array.isArray(value) ? "array" : typeof value));
    return value;
  }
  function parseLong(value) {
    const s = parsingString(value);
    if (/^[+-]?\d+$/.exec(s)?.[0] !== s) return null;
    const n = parseInt(s, 10);
    return n <= Number.MAX_SAFE_INTEGER && n >= Number.MIN_SAFE_INTEGER ? n : null;
  }
  function parseDouble(value) {
    const s = parsingString(value);
    if (/^[\x00-\x20]*[+-]?NaN[\x00-\x20]*$/.test(s)) return NaN;
    return /^[\x00-\x20]*[+-]?(Infinity|((\d+\.?\d*|\.\d+)([eE][+-]?\d+)?)[dDfF]?)[\x00-\x20]*$/.test(s) ? parseFloat(s) : null;
  }
  function parseUuid(value) {
    const s = parsingString(value);
    return s.length === 36 && /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(s) ? uuid(s) : null;
  }
  function parseBoolean(value) {
    const s = parsingString(value);
    return s === "true" ? true : s === "false" ? false : null;
  }
  function groupBy(fn, coll) {
    return reduce((result, value) => {
      const key = fn.call(null, value);
      return assoc(result, key, conj(get(result, key, vector()), value));
    }, mapFromArray([]), coll);
  }
  function frequencies(coll) {
    return reduce((result, value) => assoc(result, value, get(result, value, 0) + 1), mapFromArray([]), coll);
  }
  function updateVals(coll, fn) {
    const kind = mapKinds.get(coll);
    const initial = kind === "array-map" || kind === "hash-map" ? coll : mapFromArray([]);
    return withMeta(reduceKv((result, key, value) => assoc(result, key, fn.call(null, value)), initial, coll), meta(coll));
  }
  function updateKeys(coll, fn) {
    return withMeta(reduceKv((result, key, value) => assoc(result, fn.call(null, key), value), mapFromArray([]), coll), meta(coll));
  }
  function merge(...values) { return mergeMaps(fnRest(values, 0), missing); }
  function mergeWith(fn, ...values) { return mergeMaps(fnRest(values, 0), fn); }
  function zipmap(keys, values) {
    const entries = [];
    let ks = seq(keys), vs = seq(values);
    while (ks != null && vs != null) {
      putEntry(entries, first(ks), first(vs));
      ks = next(ks);
      vs = next(vs);
    }
    return makeMap(entries);
  }
  function set(value) {
    if (predicates.isSet(value)) return withMeta(value, null);
    const entries = [];
    for (let current = seq(value); current != null; current = next(current)) putMember(entries, first(current));
    return makeSet(entries);
  }
  function equalPair(left, right) {
    if (left == null) return right == null;
    if (left === right) return true;
    const equiv = builtin(left, "IEquiv/-equiv", 2);
    if (equiv) return equiv.call(left, left, right);
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
    if (recordTypes.get(left) !== recordTypes.get(right)) return false;
    const leftMap = maps.get(left);
    if (leftMap) {
      if (!predicates.isMap(right) || predicates.isRecord(right) || leftMap.length !== count(right)) return false;
      return leftMap.every(entry => {
        const found = get(right, entry.key, missing);
        return found !== missing && equalPair(entry.value, found);
      });
    }
    const leftSet = sets.get(left);
    if (leftSet) {
      if (!predicates.isSet(right) || leftSet.length !== count(right)) return false;
      try { return leftSet.every(entry => contains(right, entry.key)); }
      catch (error) { if (error instanceof Error) return false; throw error; }
    }
    if (sequential(left)) {
      if (!predicates.isSequential(right)) return false;
      if (satisfies(ICounted, left) && satisfies(ICounted, right) && count(left) !== count(right)) return false;
      let a = seq(left), b = seq(right);
      while (a !== null) {
        if (b === null || !equalPair(first(a), first(b))) return false;
        a = next(a); b = next(b);
      }
      return b === null;
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
    if (value == null) return 0;
    let result = 0;
    for (let i = 0; i < value.length; i++) result = Math.imul(result, 31) + value.charCodeAt(i);
    return result;
  }

  // ClojureScript's public Murmur3 collection/name hashes. Number hashes
  // follow the JavaScript Number profile, including signed zero and nonfinite values.
  function rotateLeft(value, distance) { return (value << distance) | (value >>> (32 - distance)); }
  function mixK1(value) { return Math.imul(rotateLeft(Math.imul(value, 0xcc9e2d51), 15), 0x1b873593); }
  function mixH1(hash, value) { return (Math.imul(rotateLeft(hash ^ value, 13), 5) + 0xe6546b64) | 0; }
  function fmix(hash, length) {
    hash ^= length;
    hash = Math.imul(hash ^ (hash >>> 16), 0x85ebca6b);
    hash = Math.imul(hash ^ (hash >>> 13), 0xc2b2ae35);
    return hash ^ (hash >>> 16);
  }
  function hashInt(value) { return value === 0 ? 0 : fmix(mixH1(0, mixK1(value)), 4); }
  function hashChars(value) {
    let h = 0, i = 1;
    for (; i < value.length; i += 2) h = mixH1(h, mixK1(value.charCodeAt(i - 1) | (value.charCodeAt(i) << 16)));
    if (value.length & 1) h ^= mixK1(value.charCodeAt(value.length - 1));
    return fmix(h, value.length * 2);
  }
  function hashNamed(value) {
    const seed = hashChars(value.name);
    return seed ^ (stringHash(value.namespace) + 0x9e3779b9 + (seed << 6) + (seed >> 2));
  }
  function hashDouble(value) {
    const buffer = new ArrayBuffer(8), view = new DataView(buffer);
    // Upstream reads native little-endian Float64 bytes as big-endian int32s.
    view.setFloat64(0, value, true);
    return view.getInt32(0) ^ view.getInt32(4);
  }
  function mixCollectionHash(basis, count) { return fmix(mixH1(0, mixK1(basis)), count); }
  function hashOrderedColl(value) {
    let result = 1, n = 0;
    for (let current = seq(value); current !== null; current = next(current), n++)
      result = (Math.imul(result, 31) + hash(first(current))) | 0;
    return mixCollectionHash(result, n);
  }
  function hashUnorderedColl(value) {
    let result = 0, n = 0;
    for (let current = seq(value); current !== null; current = next(current), n++)
      result = (result + hash(first(current))) | 0;
    return mixCollectionHash(result, n);
  }
  // Weak identity bookkeeping never reads or mutates foreign object fields.
  function hash(value) {
    if (value == null) return 0;
    const method = implementsProtocol(IHash, value) ? builtin(value, "IHash/-hash", 1) : null;
    if (method) return method.call(value, value) | 0;
    if (typeof value === "boolean") return value ? 1231 : 1237;
    if (typeof value === "number") {
      if (!Number.isFinite(value)) return value === Infinity ? 2146435072 : value === -Infinity ? -1048576 : 2146959360;
      return Number.isSafeInteger(value) ? (Math.floor(value) % 2147483647) | 0 : hashDouble(value);
    }
    if (typeof value === "string") return hashInt(stringHash(value));
    if (value instanceof Date) return value.valueOf() | 0;
    if (uuids.has(value)) {
      if (!uuidHashes.has(value)) uuidHashes.set(value, hash(uuids.get(value)));
      return uuidHashes.get(value);
    }
    if (vars.has(value)) return hash(vars.get(value).symbol);
    const named = keywords.get(value) || symbols.get(value);
    if (named) return (hashNamed(named) + (keywords.has(value) ? 0x9e3779b9 : 0)) | 0;
    const entries = maps.get(value);
    if (entries) {
      let result = 0;
      for (const entry of entries) result = (result + mixCollectionHash((Math.imul(31, 31 + entry.hash) + hash(entry.value)) | 0, 2)) | 0;
      return mixCollectionHash(result, entries.length);
    }
    const members = sets.get(value);
    if (members) {
      let result = 0;
      for (const entry of members) result = (result + entry.hash) | 0;
      return mixCollectionHash(result, members.length);
    }
    if (sequential(value)) {
      const cached = lazyValues.get(value) ?? cells.get(value)?.chunkedState;
      if (cached?.hash != null) return cached.hash;
      const result = hashOrderedColl(value);
      if (cached) cached.hash = result;
      return result;
    }
    const nativeHash = builtin(value, "IHash/-hash", 1);
    if (nativeHash) return nativeHash.call(value, value) | 0;
    if (typeof value === "symbol" || typeof value === "bigint") return hash(String(value));
    if (!identityHashes.has(value)) identityHashes.set(value, ++identityCounter);
    return identityHashes.get(value);
  }

  // Versioned in-process compiler bridge. Descriptors are private arrays,
  // never printed text or JSON. Only brands above authorize language values.
  function compilerFormBridge() {
    const borrowed = new WeakMap(), literals = new WeakMap(), primitives = new Map();
    const negativeZero = Object.freeze({});
    const primitiveKey = v => typeof v === "number" && Object.is(v, -0) ? negativeZero : v;
    const dateTime = Function.prototype.call.bind(Date.prototype.getTime);
    const regexSource = Function.prototype.call.bind(Object.getOwnPropertyDescriptor(RegExp.prototype, "source").get);
    const regexFlags = [["hasIndices","d"],["global","g"],["ignoreCase","i"],["multiline","m"],["dotAll","s"],["unicode","u"],["sticky","y"]]
      .map(([property, letter]) => [Object.getOwnPropertyDescriptor(RegExp.prototype, property)?.get, letter]);
    const fromUnits = units => {
      let result = "";
      for (let i = 0; i < units.length; i += 4096)
        result += String.fromCharCode(...units.slice(i, i + 4096));
      return result;
    };
    const units = text => {
      const result = [];
      for (let i = 0; i < text.length; i++) result.push(text.charCodeAt(i));
      return result;
    };
    let terminal = null;
    const fail = (category, path) => {
      const message = "Unsupported macro result " + category + " at " + path;
      if (category.includes("limit")) terminal ??= message;
      throw new Error(message);
    };
    function decode(root) {
      // Bootstrap syntax quotations can be deep. Traverse descriptors explicitly
      // so the documented form depth budget does not consume the engine stack.
      const values = new Map(), pending = [[root, false]];
      while (pending.length) {
        const [d, ready] = pending.pop();
        if (ready) { values.set(d, decodeNode(d, values)); continue; }
        pending.push([d, true]);
        const [kind, , metadata, a] = d;
        if (metadata != null) pending.push([metadata, false]);
        if (["list", "vector", "map", "set"].includes(kind)) {
          for (let i = a.length - 1; i >= 0; --i) pending.push([a[i], false]);
        } else if (["regex", "instant", "uuid", "queue", "js-array", "js-object"].includes(kind)) {
          pending.push([a, false]);
        }
      }
      return values.get(root);
    }
    function decodeNode(d, values) {
      const decode = child => values.get(child);
      const [kind, id, metadata, a, b] = d;
      let value;
      switch (kind) {
        case "nil": value = null; break;
        case "bool": case "number": value = a; break;
        case "string": case "character": value = fromUnits(a); break;
        case "symbol": case "keyword": value = (kind === "symbol" ? symbol : keyword)(a == null ? null : fromUnits(a), fromUnits(b)); if (kind === "symbol" && a == null) generatedNames.add(fromUnits(b)); break;
        case "list": value = listFromArray(a.map(decode));
          if (a.length === 0) { value = builtinValue("EmptyList", sequencePrototype); cells.set(value, cells.get(emptyList)); }
          break;
        case "vector": value = vectorFromArray(a.map(decode)); break;
        case "map": value = mapFromArray(a.map(decode)); break;
        case "set": value = setFromArray(a.map(decode)); break;
        case "regex": value = rePattern(decode(a)); break;
        case "instant": value = new Date(decode(a)); break;
        case "uuid": value = uuid(decode(a)); break;
        case "queue": value = queueFromArray(vectors.get(decode(a))); break;
        case "js-array": value = vectors.get(decode(a)).slice(); literals.set(value, "js-array"); break;
        case "js-object": {
          const payload = decode(a), pairs = [];
          for (const e of maps.get(payload)) pairs.push(typeof e.key === "string" ? e.key : name(e.key), e.value);
          value = jsObject(pairs); literals.set(value, "js-object"); break;
        }
        default: throw new Error("Unknown compiler form descriptor");
      }
      if (metadata != null) {
        const m = decode(metadata);
        // Compiler metadata uses the canonical private store, never user hooks.
        if (value == null || (typeof value !== "object" && typeof value !== "function")) throw new Error("Invalid compiler metadata target");
        metadataValues.set(value, m);
      }
      if (value != null && typeof value === "object") borrowed.set(value, id);
      else primitives.set(primitiveKey(value), id);
      return value;
    }
    function encode(value, limit, depthLimit) {
      let remaining = limit;
      const active = new WeakSet();
      function visit(v, path, depth) {
        if (--remaining < 0) fail("node limit", path);
        if (depth > depthLimit) fail("depth limit", path);
        const kind = typeof v;
        const object = v != null && (kind === "object" || kind === "function");
        if (object && active.has(v)) fail("cyclic data", path);
        if (object) active.add(v);
        try {
          let tag, a, b;
          if (v === null) tag = "nil";
          else if (kind === "boolean") { tag = "bool"; a = v; }
          else if (kind === "number") { tag = "number"; a = v; }
          else if (kind === "string") { tag = "string"; a = units(v); }
          else if (kind !== "object") fail(kind, path);
          else if (vars.has(v)) fail("Var", path);
          else if (atoms.has(v)) fail("atom", path);
          else if (volatiles.has(v)) fail("volatile", path);
          else if (typeFields.has(v) || recordTypes.has(v) || reifyValues.has(v) || ifnValues.has(v)) fail("protocol/type instance", path);
          else if (symbols.has(v) || keywords.has(v)) {
            const n = symbols.get(v) || keywords.get(v); tag = symbols.has(v) ? "symbol" : "keyword";
            a = n.namespace == null ? null : units(n.namespace); b = units(n.name);
          } else if (vectors.has(v)) { tag = "vector"; a = vectors.get(v).map((x, i) => visit(x, path + "[" + i + "]", depth + 1)); }
          else if (maps.has(v)) { tag = "map"; a = []; let i = 0;
            for (const e of maps.get(v)) { a.push(visit(e.key, path + ".key[" + i + "]", depth + 1), visit(e.value, path + ".value[" + i++ + "]", depth + 1)); }
          } else if (sets.has(v)) { tag = "set"; a = sets.get(v).map((e, i) => visit(e.key, path + "[" + i + "]", depth + 1)); }
          else if (queues.has(v)) { tag = "queue"; a = visit(vectorFromArray(queues.get(v)), path + ".payload", depth + 1); }
          else if (uuids.has(v)) { tag = "uuid"; a = visit(uuids.get(v), path + ".payload", depth + 1); }
          else if (literals.has(v)) {
            tag = literals.get(v);
            if (--remaining < 0) fail("node limit", path + ".payload");
            if (depth + 1 > depthLimit) fail("depth limit", path + ".payload");
            const data = Object.getOwnPropertyDescriptors(v), items = [];
            if (tag === "js-array") {
              const length = data.length.value;
              if (length > remaining) fail("node limit", path);
              for (let i = 0; i < length; i++) {
                const d = data[i]; if (!d || !("value" in d)) fail("accessor/hole in #js array", path + "[" + i + "]");
                items.push(visit(d.value, path + "[" + i + "]", depth + 2));
              }
            } else {
              for (const key of Reflect.ownKeys(data)) {
                if (typeof key !== "string") fail("symbol key in #js object", path);
                const d = data[key]; if (!("value" in d)) fail("accessor in #js object", path);
                items.push(visit(key, path + ".key", depth + 2), visit(d.value, path + "." + key, depth + 2));
              }
            }
            a = [tag === "js-array" ? "vector" : "map", 0, null, items];
          } else if (cells.has(v) || views.has(v) || lazyValues.has(v)) {
            tag = "list"; a = [];
            const seen = new WeakSet(); let current = v;
            while (true) {
              if (--remaining < 0) fail("sequence limit", path);
              if (current == null) break;
              if (!cells.has(current) && !views.has(current) && !lazyValues.has(current)) fail("noncanonical sequence tail", path);
              if (seen.has(current)) fail("cyclic sequence", path); seen.add(current);
              // Lazy realization is intentionally executable but shares limits.
              // Work only with private payloads. User protocol objects are not
              // expansion trees; realization cannot call their getters to diagnose.
              try {
                while (lazyValues.has(current)) {
                  if (--remaining < 0) fail("sequence limit", path);
                  current = deferredValue(lazyValues.get(current), true);
                }
              } catch (_) { fail("lazy realization failure", path); }
              if (current == null) break;
              if (vectors.has(current)) current = view(vectors.get(current), 0, true);
              if (cells.has(current)) {
                const cell = cells.get(current);
                if (cell.kind === "list" && cell.count === 0) break;
                let head, tail;
                try { head = cell.head; tail = cell.tail; } catch (_) { fail("sequence realization failure", path); }
                a.push(visit(head, path + "[" + a.length + "]", depth + 1));
                current = tail;
              } else if (views.has(current)) {
                const slice = views.get(current);
                if (slice.offset >= slice.storage.length) break;
                a.push(visit(slice.storage[slice.offset], path + "[" + a.length + "]", depth + 1));
                current = slice.offset + 1 < slice.storage.length ? view(slice.storage, slice.offset + 1, slice.indexed, slice.kind, slice.tail) : slice.tail;
              } else fail("noncanonical sequence tail", path);
            }
          } else {
            // Intrinsic brand checks avoid getters, custom toString and instanceof.
            try { const ms = dateTime(v); tag = "instant"; if (!Number.isFinite(ms)) fail("invalid instant", path); a = visit(ms, path + ".payload", depth + 1); } catch (e) { if (tag) throw e; }
            if (!tag) {
              let pattern;
              try { pattern = regexSource(v); } catch (_) {}
              if (pattern != null) {
                let flags = "";
                for (const [g, letter] of regexFlags) {
                  if (g && g.call(v)) flags += letter;
                }
                tag = "regex"; a = visit(flags ? "(?" + flags + ")" + pattern : pattern, path + ".payload", depth + 1);
              }
            }
            if (!tag) fail("arbitrary object (including promise)", path);
          }
          const metadata = object && metadataValues.has(v) ? visit(metadataValues.get(v), path + ".meta", depth + 1) : null;
          return [tag, object ? borrowed.get(v) || 0 : primitives.get(primitiveKey(v)) || 0, metadata, a, b];
        } finally { if (object) active.delete(v); }
      }
      return visit(value, "$", 0);
    }
    return Object.freeze({version: 1, decode, encode, check: () => { if (terminal != null) throw new Error(terminal); },
      // Explicit descriptors authorize host literal data; ordinary objects remain invalid.
      jsArray: items => { const v = Array.from(items); literals.set(v, "js-array"); return v; },
      jsObject: pairs => { const v = jsObject(pairs); literals.set(v, "js-object"); return v; },
      withMeta: (value, metadata) => {
        if (!literals.has(value)) throw new Error("Invalid compiler literal metadata target");
        metadataValues.set(value, metadata); return value;
      }});
  }

  // Private test adapter. Production emission keeps this factory local; only
  // the observation harness registers inspect. Exposed storage remains frozen.
  function inspect(value) {
    if (isReduced(value) || uuids.has(value) || vars.has(value) || atoms.has(value) || volatiles.has(value)) return Object.freeze({kind: "opaque"});
    const named = keywords.get(value) || symbols.get(value);
    if (named) return Object.freeze({kind: keywords.has(value) ? "keyword" : "symbol",
      namespace: named.namespace, name: named.name, spelling: named.spelling});
    const items = vectors.get(value);
    if (items) return Object.freeze({kind: "vector", items});
    const entries = recordTypes.has(value)
      ? recordEntries(value, recordTypes.get(value)).map(pair => ({key: nth(pair, 0), value: nth(pair, 1)}))
      : maps.get(value);
    if (entries) return Object.freeze({kind: "map", entries: Object.freeze(entries.map(
      entry => Object.freeze([entry.key, entry.value]))), count: entries.length});
    const members = sets.get(value);
    if (members) return Object.freeze({kind: "set",
      items: Object.freeze(members.map(entry => entry.key)), count: members.length});
    if (cells.has(value) || views.has(value)) {
      const items = [];
      for (let current = seq(value); current !== null; current = next(current)) items.push(first(current));
      return Object.freeze({kind: cells.get(value)?.kind === "list" ? "list" : "seq", items: Object.freeze(items)});
    }
    return null;
  }

  function queueFromArray(items) {
    const value = builtinValue("PersistentQueue", sequencePrototype);
    queues.set(value, Object.freeze(items.slice()));
    return value;
  }

  function jsObject(pairs) {
    const result = {};
    for (let i = 0; i < pairs.length; i += 2) {
      // #js objects have object-literal __proto__ semantics.
      if (pairs[i] === "__proto__") {
        const prototype = pairs[i + 1];
        if (prototype === null || typeof prototype === "object" || typeof prototype === "function") Object.setPrototypeOf(result, prototype);
      } else Object.defineProperty(result, pairs[i], {value: pairs[i + 1], writable: true, enumerable: true, configurable: true});
    }
    return result;
  }

  const IChunk = function () {}, IChunkedNext = function () {}, IDrop = function () {};
  const IMapEntry = function () {}, IFind = function () {}, ISorted = function () {}, IChunkedSeq = function () {};
  const IEncodeJS = function () {}, IEncodeClojure = function () {};
  const IRecord = function () {}, ICloneable = function () {}, IPrintWithWriter = function () {}, IWriter = function () {};
  const INamed = function () {};
  const IMeta = function () {}, IWithMeta = function () {}, IHash = function () {}, IEquiv = function () {};
  const IPending = function () {}, IDeref = function () {}, IAtom = function () {}, IReset = function () {}, ISwap = function () {}, IVolatile = function () {}, IWatchable = function () {};
  const Fn = function () {}, IList = function () {}, IFn = function () {}, ISeqable = function () {}, ISeq = function () {}, INext = function () {}, ICounted = function () {}, IIndexed = function () {}, ILookup = function () {}, ISequential = function () {}, IAssociative = function () {};
  const IReduce = function () {}, IKVReduce = function () {}, IVector = function () {}, IMap = function () {}, ICollection = function () {}, IEmptyableCollection = function () {}, ISet = function () {}, IStack = function () {}, IReversible = function () {}, IComparable = function () {}, IIterable = function () {};
  const IMultiFn = function () {};
  const IEditableCollection = function () {}, ITransientCollection = function () {}, ITransientAssociative = function () {}, ITransientMap = function () {}, ITransientVector = function () {}, ITransientSet = function () {};
  const coreProtocolNames = new Map([[INamed, "INamed"], [IMultiFn, "IMultiFn"], [IEditableCollection, "IEditableCollection"], [ITransientCollection, "ITransientCollection"], [ITransientAssociative, "ITransientAssociative"], [ITransientMap, "ITransientMap"], [ITransientVector, "ITransientVector"], [ITransientSet, "ITransientSet"], [IEncodeJS, "IEncodeJS"], [IEncodeClojure, "IEncodeClojure"], [IChunk, "IChunk"], [IChunkedNext, "IChunkedNext"], [IDrop, "IDrop"], [IPending, "IPending"], [Fn, "Fn"], [IList, "IList"], [IMapEntry, "IMapEntry"], [IFind, "IFind"], [ISorted, "ISorted"], [IChunkedSeq, "IChunkedSeq"], [IRecord, "IRecord"], [ICloneable, "ICloneable"], [IPrintWithWriter, "IPrintWithWriter"], [IWriter, "IWriter"], [IReduce, "IReduce"], [IKVReduce, "IKVReduce"], [IVector, "IVector"], [IMap, "IMap"], [ICollection, "ICollection"], [IEmptyableCollection, "IEmptyableCollection"], [ISet, "ISet"], [IStack, "IStack"], [IReversible, "IReversible"], [IComparable, "IComparable"], [IIterable, "IIterable"], [IHash, "IHash"], [IEquiv, "IEquiv"], [IFn, "IFn"], [ISeqable, "ISeqable"], [ISeq, "ISeq"], [INext, "INext"], [ICounted, "ICounted"], [IIndexed, "IIndexed"], [ILookup, "ILookup"], [ISequential, "ISequential"], [IAssociative, "IAssociative"], [IMeta, "IMeta"], [IWithMeta, "IWithMeta"], [IDeref, "IDeref"], [IAtom, "IAtom"], [IReset, "IReset"], [ISwap, "ISwap"], [IVolatile, "IVolatile"], [IWatchable, "IWatchable"]]);
  const coreProtocolsByName = new Map([...coreProtocolNames].map(([protocol, name]) => [name, protocol]));
  IMeta.function = true; Fn.function = true; ICounted.null = true; IHash._ = true; IEquiv._ = true;
  // Metadata belongs to values, never structural equality or hash payloads.
  function meta(value) {
    const method = value == null ? null : publicMethod(value, "IMeta/-meta", 1);
    if (method) return method.call(value, value);
    return atoms.has(value) ? value.meta : metadataValues.has(value) ? metadataValues.get(value) : null;
  }
  function empty(value) {
    const method = value == null ? null : publicMethod(value, "IEmptyableCollection/-empty", 1);
    if (method) return method.call(value, value);
    if (value == null) return null;
    if (recordTypes.has(value) || mapEntries.has(value)) return null;
    if (vectors.has(value)) return preserveMeta(value, vector());
    if (maps.has(value)) return preserveMeta(value, makeMap([], mapKinds.get(value), sortedComparators.get(value)));
    if (sets.has(value)) return preserveMeta(value, makeSet([], setKinds.get(value), sortedComparators.get(value)));
    if (queues.has(value)) return preserveMeta(value, queueFromArray([]));
    if (cells.has(value) || views.has(value) || lazyValues.has(value))
      return cells.get(value)?.kind === "list" || views.get(value)?.kind === "queue" || lazyValues.has(value) ? withMeta(emptyList, meta(value)) : emptyList;
    return null;
  }
  function peek(value) {
    const method = value == null ? null : builtin(value, "IStack/-peek", 1);
    if (method) return method.call(value, value);
    if (value == null) return null;
    if (vectors.has(value)) { const items = vectors.get(value); return items.length ? items[items.length - 1] : null; }
    if (queues.has(value) || cells.get(value)?.kind === "list") return first(value);
    throw new Error("No protocol method IStack.-peek defined for this value");
  }
  function pop(value) {
    const method = value == null ? null : builtin(value, "IStack/-pop", 1);
    if (method) return method.call(value, value);
    if (value == null) return null;
    if (vectors.has(value)) {
      const items = vectors.get(value);
      if (!items.length) throw new Error("Can't pop empty vector");
      return preserveMeta(value, makeVector(items.slice(0, -1)));
    }
    if (queues.has(value)) return preserveMeta(value, queueFromArray(queues.get(value).slice(1)));
    if (cells.get(value)?.kind === "list") {
      if (!count(value)) throw new Error("Can't pop empty list");
      return rest(value);
    }
    throw new Error("No protocol method IStack.-pop defined for this value");
  }
  function rseq(value) {
    const method = builtin(value, "IReversible/-rseq", 1);
    if (method) return method.call(value, value);
    if (!vectors.has(value) && views.get(value)?.kind !== "indexed" && !sortedComparators.has(value))
      throw new Error("No protocol method IReversible.-rseq defined for this value");
    const items = Object.freeze(toArray(value).reverse());
    return items.length ? view(items, 0, false, maps.has(value) ? "sorted-map" : "reverse") : null;
  }
  function mapProjection(value, index, alreadySeq = false) {
    const entries = alreadySeq ? value : seq(value);
    if (entries === null) return null;
    const result = Object.create(sequencePrototype);
    cells.set(result, Object.freeze({kind: index === 0 ? "keys" : "vals",
      get head() { const entry = core.firstProtocol(entries); return index === 0 ? core.keyProtocol(entry) : core.valProtocol(entry); },
      get tail() {
        const following = satisfies(INext, entries) ? core.nextProtocol(entries) : next(entries);
        return following == null ? emptyList : mapProjection(following, index, true);
      }}));
    return result;
  }
  function keys(value) { return mapProjection(value, 0); }
  function vals(value) { return mapProjection(value, 1); }
  function repeat(n, item) {
    if (arguments.length === 1) { item = n; n = -1; }
    if (arguments.length < 1 || arguments.length > 2) return invalidArity(arguments.length);
    if (arguments.length === 2 && !(n > 0)) return emptyList;
    const value = builtinValue("Repeat", sequencePrototype);
    let following;
    cells.set(value, Object.freeze({kind: "repeat", remaining: n, head: item, get tail() { return n === -1 ? value : (following ??= n > 1 ? repeat(n - 1, item) : emptyList); }}));
    return value;
  }
  function cycleNode(all, previous, current, following = null) {
    const value = builtinValue("Cycle", sequencePrototype), state = {current, following};
    const currentValue = () => state.current ?? (state.current = next(previous) ?? all);
    cells.set(value, Object.freeze({kind: "cycle", all, previous, state, currentValue,
      get realized() { return state.current != null; },
      get head() { return first(currentValue()); },
      get tail() { return state.following ?? (state.following = cycleNode(all, currentValue(), null)); }}));
    return value;
  }
  function cycle(collection) {
    const start = seq(collection);
    return start === null ? emptyList : cycleNode(start, null, start);
  }
  function compareKeys(a, b) {
    if (a === b) return 0;
    if (a == null) return -1;
    if (b == null) return 1;
    const cannotCompare = () => { throw new Error("Cannot compare " + str(a) + " to " + str(b)); };
    if (typeof a === "number") return typeof b === "number" ? (a < b ? -1 : a > b ? 1 : 0) : cannotCompare();
    const method = builtin(a, "IComparable/-compare", 2);
    if (method) return method.call(a, a, b);
    if (uuids.has(a)) return uuids.has(b) ? compareKeys(uuids.get(a), uuids.get(b)) : cannotCompare();
    if (a instanceof Date) return b instanceof Date ? compareKeys(a.valueOf(), b.valueOf()) : cannotCompare();
    const left = keywords.get(a) || symbols.get(a), right = keywords.get(b) || symbols.get(b);
    if (left || right) {
      if (!left || !right || keywords.has(a) !== keywords.has(b)) throw new Error("Cannot compare different types");
      return compareKeys(left.namespace, right.namespace) || compareKeys(left.name, right.name);
    }
    if (vectors.has(a) && vectors.has(b)) {
      const xs = vectors.get(a), ys = vectors.get(b);
      if (xs.length !== ys.length) return xs.length < ys.length ? -1 : 1;
      for (let i = 0; i < xs.length; i++) { const order = compareKeys(xs[i], ys[i]); if (order) return order; }
      return 0;
    }
    if (!(Array.isArray(a) && Array.isArray(b)) && (typeof a !== typeof b || !["string", "boolean"].includes(typeof a))) return cannotCompare();
    return a < b ? -1 : a > b ? 1 : 0;
  }
  function sequentialIndex(coll, value, start, reverse) {
    const length = count(coll);
    if (reverse) {
      if (length === 0) return -1;
      for (let i = start > 0 ? Math.min(length - 1, start) : start < 0 ? length + start : start; i >= 0; i--)
        if (equalPair(nth(coll, i), value)) return i;
    } else {
      for (let i = start < 0 ? Math.max(0, length + start) : start; i < length; i++)
        if (equalPair(nth(coll, i), value)) return i;
    }
    return -1;
  }
  // Comparator equality controls sorted membership; structural hashes remain
  // attached to the retained key for unordered collection equality/hash.
  function treeEntry(node) {
    return mapEntries.has(node) || vectors.has(node) ? node : mapEntry(node.key, node.val);
  }
  function treeMapConstructor(comparator, tree, _count, metadata, _hash) {
    const entries = [];
    function visit(node) {
      if (node == null) return;
      visit(node.left);
      const entry = treeEntry(node);
      putSortedEntry(entries, nth(entry, 0), nth(entry, 1), comparator);
      visit(node.right);
    }
    visit(tree);
    return withMeta(makeMap(entries, "sorted-map", comparator), metadata);
  }
  function treeSetConstructor(metadata, treeMap, _hash) {
    if (!sortedComparators.has(treeMap) || !maps.has(treeMap)) throw new Error("Expected sorted map");
    return withMeta(makeSet(maps.get(treeMap).map(entry => Object.freeze({key: entry.key, get hash() { return entry.hash; }})), "sorted-set", sortedComparators.get(treeMap)), metadata);
  }
  function treeMapSeqConstructor(metadata, stack, ascending, _count, _hash) {
    const entries = [], pending = toArray(stack);
    while (pending.length) {
      const node = pending.shift();
      entries.push(treeEntry(node));
      let branch = truth(ascending) ? node.right : node.left;
      while (branch != null) { pending.unshift(branch); branch = truth(ascending) ? branch.left : branch.right; }
    }
    return withMeta(view(Object.freeze(entries), 0, false, "sorted-map"), metadata);
  }
  function sortedPosition(entries, key, comparator) {
    let lo = 0, hi = entries.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      if (comparator.call(null, entries[mid].key, key) < 0) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }
  function collectionEntryIndex(value, entries, key) {
    const comparator = sortedComparators.get(value);
    if (!comparator) return entryIndex(entries, key, hash(key));
    const i = sortedPosition(entries, key, comparator);
    return i < entries.length && comparator.call(null, entries[i].key, key) === 0 ? i : -1;
  }
  function putSortedEntry(entries, key, value, comparator, member = false) {
    const i = sortedPosition(entries, key, comparator);
    const exists = i < entries.length && comparator.call(null, entries[i].key, key) === 0;
    if (exists && (member || equalPair(entries[i].value, value))) return;
    const retained = exists ? entries[i].key : key;
    const entry = Object.freeze({key: retained, value, get hash() { return hash(retained); }});
    if (exists) entries[i] = entry;
    else entries.splice(i, 0, entry);
  }
  function putCollectionEntry(coll, entries, key, value, member = false) {
    const comparator = sortedComparators.get(coll);
    if (comparator) putSortedEntry(entries, key, value, comparator, member);
    else if (member) putMember(entries, key);
    else putEntry(entries, key, value);
  }
  function buildSortedMap(comparator, pairs) {
    const entries = [];
    for (let i = 0; i < pairs.length; i += 2) putSortedEntry(entries, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null, comparator);
    return makeMap(entries, "sorted-map", comparator);
  }
  function sortedMap(...pairs) { return buildSortedMap(compareKeys, pairs); }
  function sortedMapBy(comparator, ...pairs) {
    return buildSortedMap(fnComparator(comparator), pairs);
  }
  function buildSortedSet(comparator, items) {
    const entries = [];
    for (const item of items) putSortedEntry(entries, item, undefined, comparator, true);
    return makeSet(entries, "sorted-set", comparator);
  }
  function sortedSet(...items) { return buildSortedSet(compareKeys, items); }
  function sortedSetBy(comparator, ...items) {
    return buildSortedSet(fnComparator(comparator), items);
  }
  function sortedSeqFrom(value, key, ascending) {
    const entries = maps.get(value) ?? sets.get(value), comparator = sortedComparators.get(value);
    let i = sortedPosition(entries, key, comparator);
    const absent = () => sets.has(value) ? mapSeq(predicates.identity, null) : null;
    if (!truth(ascending)) {
      if (i === entries.length || comparator.call(null, entries[i].key, key) !== 0) i--;
      if (i < 0) return absent();
    } else if (i === entries.length) return absent();
    const selected = truth(ascending) ? entries.slice(i) : entries.slice(0, i + 1).reverse();
    return sets.has(value) ? mapSeq(entry => entry.key, selected) : view(Object.freeze(selected.map(entry => mapEntry(entry.key, entry.value))), 0, false, "sorted-map");
  }
  function mkBoundFn(sc, test, key) {
    if (arguments.length !== 3) return invalidArity(arguments.length);
    return entry => truth(test.call(null, core.comparatorProtocol(sc).call(null, core.entryKeyProtocol(sc, entry), key), 0));
  }
  function subseq(sc, test, key, endTest, endKey) {
    if (arguments.length !== 3 && arguments.length !== 5) return invalidArity(arguments.length);
    const include = mkBoundFn(sc, test, key);
    if (arguments.length === 3 && test !== numeric.greater && test !== numeric.greaterEqual)
      return takeWhile(include, core.sortedSeqProtocol(sc, true));
    let s = core.sortedSeqFromProtocol(sc, key, true);
    if (s == null) return null;
    if (!include(first(s))) s = next(s);
    return arguments.length === 3 ? s : takeWhile(mkBoundFn(sc, endTest, endKey), s);
  }
  function rsubseq(sc, test, key, endTest, endKey) {
    if (arguments.length !== 3 && arguments.length !== 5) return invalidArity(arguments.length);
    if (arguments.length === 3 && test !== numeric.less && test !== numeric.lessEqual)
      return takeWhile(mkBoundFn(sc, test, key), core.sortedSeqProtocol(sc, false));
    const include = arguments.length === 3 ? mkBoundFn(sc, test, key) : mkBoundFn(sc, endTest, endKey);
    let s = core.sortedSeqFromProtocol(sc, arguments.length === 3 ? key : endKey, false);
    if (s == null) return null;
    if (!include(first(s))) s = next(s);
    return arguments.length === 3 ? s : takeWhile(mkBoundFn(sc, test, key), s);
  }
  function metaSupported(value) {
    return typeof value === "function" || !!builtin(value, "IMeta/-meta", 1) || [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, atoms, metaFunctions].some(store => store.has(value)) || reifyValues.has(value);
  }
  function withMetaSupported(value) {
    return !!builtin(value, "IWithMeta/-with-meta", 2) || [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, metaFunctions].some(store => store.has(value)) || reifyValues.has(value);
  }
  let declarationDemand = null;
  const declarationTransactions = [];
  function setDeclarationDemand(demand) { declarationDemand = demand; }
  function demandProtocol(key, value) {
    if (!declarationDemand) return;
    const name = typeNames.get(value?.constructor);
    declarationDemand(key, value, name);
  }
  function declarationReceiver(value) { demandProtocol("Object", value); return value; }
  function patchDeclaration(value, key, descriptor) {
    const transaction = declarationTransactions.at(-1);
    if (transaction) {
      const old = Object.getOwnPropertyDescriptor(value, key);
      transaction.push({undo: () => old ? Object.defineProperty(value, key, old) : Reflect.deleteProperty(value, key), redo: () => Object.defineProperty(value, key, descriptor), snapshot: () => { const saved = Object.getOwnPropertyDescriptor(value, key); return () => saved ? Object.defineProperty(value, key, saved) : Reflect.deleteProperty(value, key); }});
    }
    Object.defineProperty(value, key, descriptor);
  }
  function setDeclaration(store, key, value) {
    const transaction = declarationTransactions.at(-1), present = store.has(key), previous = store.get(key);
    if (transaction) transaction.push({undo: () => present ? store.set(key, previous) : store.delete(key), redo: () => store.set(key, value), snapshot: () => { const saved = store.get(key), has = store.has(key); return () => has ? store.set(key, saved) : store.delete(key); }});
    store.set(key, value);
  }
  function declarationTransaction(run, capture = false) {
    const undo = [];
    declarationTransactions.push(undo);
    try { const value = run(); return capture ? {value, patches: undo} : value; }
    catch (error) {
      for (let i = undo.length - 1; i >= 0; i--) undo[i].undo();
      throw error;
    } finally { declarationTransactions.pop(); }
  }
  function applyDeclarationPatches(patches, forward) {
    if (forward) for (const patch of patches) patch.redo();
    else for (let i = patches.length - 1; i >= 0; i--) patches[i].undo();
  }
  function snapshotDeclarationPatches(patches) { return patches.map(patch => patch.snapshot()); }
  function restoreDeclarationPatches(saved) { for (let i = saved.length - 1; i >= 0; i--) saved[i](); }
  function protocolPredicate(protocol, key, value, direct) {
    demandProtocol(key, value);
    if (directMethod(value, key, 0)) return true;
    if (direct) return false;
    return !!(protocol[nativeType(value)] || protocol._);
  }
  function implementsProtocol(protocol, value) {
    const key = protocolNames.get(protocol);
    if (key) return !!directMethod(value, key, 0);
    return coreProtocolImplementation(protocol, value);
  }
  function satisfies(protocol, value) {
    demandProtocol(protocolNames.get(protocol) ?? coreProtocolNames.get(protocol), value);
    if (implementsProtocol(protocol, value)) return true;
    const key = protocolNames.get(protocol);
    if (key) return !!(protocol[nativeType(value)] || protocol._);
    if (ownedProtocolValue(value)) return false;
    const coreName = coreProtocolNames.get(protocol);
    const index = protocolMaskNames.lastIndexOf(coreName);
    if (index >= 0 && value?.["cljs$lang$protocol_mask$partition" + (index >> 5) + "$"]) return false;
    return coreName ? !!(protocol[nativeType(value)] || protocol._ || nativeMethods.get(coreName)?.get(nativeType(value)) || nativeMethods.get(coreName)?.get("_")) : false;
  }
  const protocolMaskNames = "IFn ICounted IEmptyableCollection ICollection IIndexed ASeq ISeq INext ILookup IAssociative IMap IMapEntry ISet IStack IVector IDeref IDerefWithTimeout IMeta IWithMeta IReduce IKVReduce IEquiv IHash ISeqable ISequential IList IRecord IReversible ISorted IPrintWithWriter IWriter IPrintWithWriter IPending IWatchable IEditableCollection ITransientCollection ITransientAssociative ITransientMap ITransientVector ITransientSet IMultiFn IChunkedSeq IChunkedNext IComparable INamed ICloneable IAtom IReset ISwap IIterable IDrop".split(" ");
  function cloneablePayload(value) { return (vectors.has(value) && !mapEntries.has(value)) || (maps.has(value) && !recordTypes.has(value)) || sets.has(value) || queues.has(value) || ["list", "cons", "range"].includes(cells.get(value)?.kind) || ["indexed", "reverse"].includes(views.get(value)?.kind); }
  function coreProtocolImplementation(protocol, value) {
    const name = coreProtocolNames.get(protocol);
    const maskIndex = protocolMaskNames.lastIndexOf(name);
    if (maskIndex >= 0 && (value?.["cljs$lang$protocol_mask$partition" + (maskIndex >> 5) + "$"] & (1 << (maskIndex % 32)))) return true;
    if (name && directMethod(value, name, 0)) return true;
    return builtinCoreImplementation(protocol, value);
  }
  function builtinCoreImplementation(protocol, value) {
    if (cells.get(value)?.kind === "chunked-cons" && [IReduce, IIterable, IStack].includes(protocol)) return false;
    if (protocol === INamed) return keywords.has(value) || symbols.has(value) || multiFns.has(value);
    if (protocol === IMultiFn) return multiFns.has(value);
    if (protocol === IFn && multiFns.has(value)) return true;
    const transient = transients.get(value);
    if (protocol === IEditableCollection) return vectors.has(value) && !mapEntries.has(value) && !subvecSources.has(value) || maps.has(value) && !recordTypes.has(value) && !sortedComparators.has(value) || sets.has(value) && !sortedComparators.has(value);
    if (protocol === ITransientCollection) return !!transient;
    if (protocol === ITransientAssociative) return !!transient && transient.kind !== "set";
    if (protocol === ITransientVector || protocol === IIndexed && transient) return transient?.kind === "vector";
    if (protocol === ITransientMap) return transient?.kind === "map";
    if (protocol === ITransientSet) return transient?.kind === "set";
    if (transient && (protocol === ICounted || protocol === ILookup || protocol === IFn)) return true;
    if (transient) return false;
    if (protocol === Fn) return metaFunctions.has(value) || vars.has(value);
    if (protocol === IList) return ["list", "cons"].includes(cells.get(value)?.kind);
    if (protocol === IFn) return !!ifnMethods(value) || isIdent(value) || vectors.has(value) || (maps.has(value) && !recordTypes.has(value)) || sets.has(value) || vars.has(value);
    if (protocol === IChunk) return chunks.has(value);
    if (protocol === IChunkedNext) return ["range", "chunked-cons"].includes(cells.get(value)?.kind) || views.get(value)?.kind === "chunked";
    if (protocol === IDrop) return mapKinds.get(value) === "array-map" || cells.get(value)?.integer === true || cells.get(value)?.kind === "repeat" || vectors.has(value) || ["indexed", "chunked", "array-map"].includes(views.get(value)?.kind);
    if (protocol === IReduce) return chunks.has(value) && !chunks.get(value).integer || vectors.has(value) || (maps.has(value) && !recordTypes.has(value)) || cells.has(value) || lazyValues.has(value) || (views.has(value) && views.get(value).kind !== "queue");
    if (protocol === IKVReduce) return vectors.has(value) || maps.has(value);
    if (protocol === IPrintWithWriter || protocol === IHash || protocol === IEquiv) return ownedProtocolValue(value);
    if (protocol === IMapEntry) return mapEntries.has(value);
    if (protocol === IFind) return vectors.has(value) || (maps.has(value) && !recordTypes.has(value));
    if (protocol === ISorted) return sortedComparators.has(value);
    if (protocol === IChunkedSeq) return ["range", "chunked-cons"].includes(cells.get(value)?.kind) || views.get(value)?.kind === "chunked";
    if (protocol === ICloneable) return cloneablePayload(value);
    if (protocol === IVector) return vectors.has(value);
    if (protocol === IMap) return maps.has(value);
    if (protocol === ISet) return sets.has(value);
    if (protocol === ICollection) return [vectors, maps, sets, cells, views, queues, lazyValues].some(store => store.has(value));
    if (protocol === IEmptyableCollection) return !recordTypes.has(value) && builtinCoreImplementation(ICollection, value);
    if (protocol === IStack) return vectors.has(value) || cells.has(value) || queues.has(value);
    if (protocol === IReversible) return vectors.has(value) || views.get(value)?.kind === "indexed" || sortedComparators.has(value);
    if (protocol === IIterable) return builtinCoreImplementation(ICollection, value);
    if (protocol === IComparable) return vectors.has(value) || uuids.has(value) || symbols.has(value) || keywords.has(value) || value instanceof Date;
    if (protocol === ISeqable) return [vectors, maps, sets, cells, views, queues, lazyValues].some(store => store.has(value));
    if (protocol === ISeq || protocol === INext) return protocol === ISeq && chunks.get(value)?.integer === true || cells.has(value) || views.has(value) || queues.has(value) || lazyValues.has(value);
    if (protocol === ISequential) return vectors.has(value) || cells.has(value) || views.has(value) || queues.has(value) || lazyValues.has(value);
    if (protocol === ICounted) return chunkBuffers.has(value) || chunks.has(value) || cells.get(value)?.integer === true || vectors.has(value) || maps.has(value) || sets.has(value) || queues.has(value) || cells.get(value)?.kind === "list" || (views.has(value) && ["indexed", "array-map", "sorted-map", "chunked"].includes(views.get(value).kind));
    if (protocol === IIndexed) return chunks.has(value) || cells.get(value)?.integer === true || vectors.has(value) || views.get(value)?.indexed === true;
    if (protocol === ILookup) return vectors.has(value) || maps.has(value) || sets.has(value);
    if (protocol === IAssociative) return vectors.has(value) || maps.has(value);
    if (protocol === IMeta) return [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, atoms, metaFunctions].some(store => store.has(value)) || reifyValues.has(value);
    if (protocol === IWithMeta) return [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, metaFunctions].some(store => store.has(value)) || reifyValues.has(value);
    if (protocol === IAtom) return atoms.has(value) || !!directMethod(value, "IAtom", 0);
    if (protocol === IPending) return ["repeat", "cycle", "iterate"].includes(cells.get(value)?.kind) || delays.has(value) || lazyValues.has(value);
    if (protocol === IDeref) return delays.has(value) || vars.has(value) || atoms.has(value) || volatiles.has(value) || reducedValues.has(value);
    if (protocol === IReset) return false;
    if (protocol === ISwap) return false;
    if (protocol === IVolatile) return volatiles.has(value);
    if (protocol === IWatchable) return atoms.has(value);
    return false;
  }
  function metaProtocol(value) {
    if (!metaSupported(value)) throw new Error("No protocol method IMeta.-meta defined for this value");
    return meta(value);
  }
  function withMetaProtocol(value, metadata) {
    if (!withMetaSupported(value)) throw new Error("No protocol method IWithMeta.-with-meta defined for this value");
    return withMeta(value, metadata);
  }
  function preserveMeta(original, result) {
    const metadata = meta(original);
    if (metadata != null || metadataValues.has(original)) metadataValues.set(result, metadata);
    return result;
  }
  function varyMeta(value, fn, ...args) { return withMeta(value, apply(fn, meta(value), args)); }
  // CLJS reference metadata operations assign .meta; Var uses a separate
  // immutable _meta, so reset-meta! does not change what (meta #'x) observes.
  function resetMeta(value, metadata) { value.meta = metadata; return metadata; }
  function alterMeta(value, fn, ...args) { return resetMeta(value, apply(fn, value.meta, args)); }
  class Atom {
    equiv(other) { return this === other; }
    constructor(state, meta, validator, watches) {
      this.state = state; this.meta = meta; this.validator = validator; this.watches = watches;
      atoms.set(this, this);
    }
  }
  class Volatile {
    constructor(state) { this.state = state; volatiles.set(this, this); }
  }
  function atomFactory(state, meta, validator, watches) { return new Atom(state, meta, validator, watches); }
  function atom(state, ...options) {
    const opts = seqToMap(listFromArray(options));
    // Construction does not validate the initial state in ClojureScript.
    return new Atom(state, get(opts, keyword(null, "meta")), get(opts, keyword(null, "validator")), null);
  }
  // Hierarchies are ordinary immutable language maps. The global reference,
  // multimethods and caches belong to this runtime factory (one phase/session).
  let globalHierarchy = null;
  const multiFns = new WeakSet();
  function makeHierarchy() {
    if (arguments.length) return invalidArity(arguments.length);
    return mapFromArray([keyword(null, "parents"), mapFromArray([]), keyword(null, "ancestors"), mapFromArray([]), keyword(null, "descendants"), mapFromArray([])]);
  }
  function getGlobalHierarchy() {
    if (arguments.length) return invalidArity(arguments.length);
    return globalHierarchy ??= atom(makeHierarchy());
  }
  function hierarchyPart(h, part) { return get(h, keyword(null, part)); }
  function bases(value) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    if (value == null || value === false) return null;
    const base = Object.getPrototypeOf(value);
    return base == null ? null : listFromArray([base]);
  }
  function supers(value) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    const result = [];
    for (let pending = bases(value); pending != null; pending = bases(first(pending))) result.push(first(pending));
    return result.length ? setFromArray(result) : null;
  }
  function isa(h, child, parent) {
    if (arguments.length === 2) return isa(deref(getGlobalHierarchy()), h, child);
    if (arguments.length !== 3) return invalidArity(arguments.length);
    if (equalPair(child, parent)) return true;
    if (typeof parent === "function" && typeof child === "function" && child instanceof parent) return true;
    const ancestors = hierarchyPart(h, "ancestors");
    if (contains(get(ancestors, child), parent)) return true;
    if (typeof child === "function") for (const base of toArray(supers(child))) if (contains(get(ancestors, base), parent)) return true;
    const cv = vectors.get(child), pv = vectors.get(parent);
    return !!(cv && pv && cv.length === pv.length && cv.every((v, i) => isa(h, v, pv[i])));
  }
  function hierarchyRel(part, h, tag, arity) {
    if (arity === 1) { tag = h; h = deref(getGlobalHierarchy()); }
    else if (arity !== 2) return invalidArity(arity);
    let result = get(hierarchyPart(h, part), tag);
    if (typeof tag === "function") {
      if (part === "descendants") throw new Error("Can't get descendants of constructors");
      const prototypes = toArray(part === "parents" ? bases(tag) : supers(tag));
      const values = toArray(result).concat(prototypes);
      if (part === "ancestors") for (const base of prototypes) values.push(...toArray(get(hierarchyPart(h, part), base)));
      result = setFromArray(values);
    }
    return count(result) ? result : null;
  }
  function parents(h, tag) { return hierarchyRel("parents", h, tag, arguments.length); }
  function ancestors(h, tag) { return hierarchyRel("ancestors", h, tag, arguments.length); }
  function descendants(h, tag) { return hierarchyRel("descendants", h, tag, arguments.length); }
  function derive(h, tag, parent) {
    if (arguments.length === 2) {
      parent = tag; tag = h;
      if (!isIdent(parent) || !namespace(parent) || !(typeof tag === "function" || isIdent(tag) && namespace(tag))) throw new Error("Assert failed: qualified derive tags");
      const ref = getGlobalHierarchy(); reset(ref, derive(deref(ref), tag, parent)); return null;
    }
    if (arguments.length !== 3) return invalidArity(arguments.length);
    if (equalPair(tag, parent) || !(typeof tag === "function" || isIdent(tag)) || !isIdent(parent)) throw new Error("Assert failed: derive tags");
    const p = hierarchyPart(h, "parents"), a = hierarchyPart(h, "ancestors"), d = hierarchyPart(h, "descendants");
    if (contains(get(p, tag), parent)) return h;
    if (contains(get(a, tag), parent)) throw new Error(print(tag, false) + "already has" + print(parent, false) + "as ancestor");
    if (contains(get(a, parent), tag)) throw new Error("Cyclic derivation:" + print(parent, false) + "has" + print(tag, false) + "as ancestor");
    function closure(table, source, sources, target, targets) {
      let result = table;
      for (const key of [source, ...toArray(get(sources, source))]) result = assoc(result, key, setFromArray([...toArray(get(targets, key)), target, ...toArray(get(targets, target))]));
      return result;
    }
    return mapFromArray([keyword(null, "parents"), assoc(p, tag, conj(get(p, tag, setFromArray([])), parent)), keyword(null, "ancestors"), closure(a, tag, d, parent, a), keyword(null, "descendants"), closure(d, parent, a, tag, d)]);
  }
  function underive(h, tag, parent) {
    if (arguments.length === 2) { const ref = getGlobalHierarchy(); reset(ref, underive(deref(ref), h, tag)); return null; }
    if (arguments.length !== 3) return invalidArity(arguments.length);
    const p = hierarchyPart(h, "parents");
    if (!contains(get(p, tag), parent)) return h;
    const remaining = disj(get(p, tag), parent), updated = count(remaining) ? assoc(p, tag, remaining) : dissoc(p, tag);
    let result = makeHierarchy();
    for (const entry of toArray(updated)) for (const ancestor of toArray(nth(entry, 1))) result = derive(result, nth(entry, 0), ancestor);
    return result;
  }
  class MultiFn {
    constructor(name, dispatchFn, defaultDispatchVal, hierarchy, methodTable, preferTable, methodCache, cachedHierarchy) {
      this.name = name; this.dispatch_fn = dispatchFn; this.default_dispatch_val = defaultDispatchVal;
      this.hierarchy = hierarchy; this.method_table = methodTable; this.prefer_table = preferTable;
      this.method_cache = methodCache; this.cached_hierarchy = cachedHierarchy;
      multiFns.add(this);
    }
    call(_self, ...args) {
      const dispatch = this.dispatch_fn.call(null, ...args), target = core.getMethod(this, dispatch);
      if (!truth(target)) throw new Error("No method in multimethod '" + print(this.name, false) + "' for dispatch value: " + print(dispatch, false));
      return target.call(null, ...args);
    }
    apply(_self, args) { return this.call(null, ...Array.from(args)); }
  }
  function demandMulti(mf) { if (declarationDemand) declarationDemand("multifn:" + print(mf.name, false), mf); }
  function multiState(ref, value) {
    // Journal compiler declaration table/cache writes for failed bundles and
    // source-frontier rewinds; ordinary runtime mutations retain source order.
    const transaction = declarationTransactions.at(-1);
    if (transaction && atoms.has(ref)) {
      const previous = ref.state;
      transaction.push({undo: () => { ref.state = previous; }, redo: () => { ref.state = value; }, snapshot: () => { const saved = ref.state; return () => { ref.state = saved; }; }});
    }
    return reset(ref, value);
  }
  function resetMultiCache(mf) {
    multiState(mf.method_cache, deref(mf.method_table));
    return multiState(mf.cached_hierarchy, deref(mf.hierarchy));
  }
  function resetMulti(mf) {
    demandMulti(mf);
    multiState(mf.method_table, mapFromArray([])); multiState(mf.method_cache, mapFromArray([]));
    multiState(mf.prefer_table, mapFromArray([])); multiState(mf.cached_hierarchy, null); return mf;
  }
  function addMethod(mf, dispatch, method) {
    // Bundle installation must not demand itself or later method declarations.
    if (!declarationTransactions.length) demandMulti(mf);
    multiState(mf.method_table, assoc(deref(mf.method_table), dispatch, method)); resetMultiCache(mf); return mf;
  }
  function removeMethod(mf, dispatch) {
    demandMulti(mf); multiState(mf.method_table, dissoc(deref(mf.method_table), dispatch)); resetMultiCache(mf); return mf;
  }
  function multiPrefers(mf, x, y) {
    // The pinned CLJS prefers* loops do not return recursive matches: only the
    // direct preference is effective, unlike Clojure's inherited preference.
    return truth(get(get(deref(mf.prefer_table), x), y));
  }
  function preferMethod(mf, x, y) {
    demandMulti(mf);
    if (multiPrefers(mf, y, x)) throw new Error("Preference conflict in multimethod '" + print(mf.name, false) + "': " + print(y, false) + " is already preferred to " + print(x, false));
    const table = deref(mf.prefer_table);
    multiState(mf.prefer_table, assoc(table, x, conj(get(table, x, setFromArray([])), y)));
    return resetMultiCache(mf);
  }
  function getMethod(mf, dispatch) {
    demandMulti(mf);
    for (;;) {
      const h = deref(mf.hierarchy);
      if (!equalPair(deref(mf.cached_hierarchy), h)) resetMultiCache(mf);
      const cached = get(deref(mf.method_cache), dispatch);
      if (truth(cached)) return cached;
      let best = null;
      const dominates = (x, y) => multiPrefers(mf, x, y) || isa(h, x, y);
      for (const entry of toArray(deref(mf.method_table))) {
        const key = nth(entry, 0);
        if (!isa(h, dispatch, key)) continue;
        const candidate = best === null || dominates(key, nth(best, 0)) ? entry : best;
        if (!dominates(nth(candidate, 0), key)) throw new Error("Multiple methods in multimethod '" + print(mf.name, false) + "' match dispatch value: " + print(dispatch, false) + " -> " + print(key, false) + " and " + print(nth(candidate, 0), false) + ", and neither is preferred");
        best = candidate;
      }
      const target = best === null ? get(deref(mf.method_table), mf.default_dispatch_val) : nth(best, 1);
      if (!truth(target)) return null;
      if (!equalPair(deref(mf.cached_hierarchy), deref(mf.hierarchy))) { resetMultiCache(mf); continue; }
      multiState(mf.method_cache, assoc(deref(mf.method_cache), dispatch, target)); return target;
    }
  }

  function stateProtocol(value, key, args) {
    const method = builtin(value, key, args.length + 1);
    if (!method) throw new Error("No protocol method " + key + " defined for this value");
    return method.call(value, value, ...args);
  }
  function resetProtocol(value, state) { return stateProtocol(value, "IReset/-reset!", [state]); }
  function swapProtocol(value, fn, ...args) {
    if (arguments.length < 2 || arguments.length > 5) return invalidArity(arguments.length);
    return stateProtocol(value, "ISwap/-swap!", [fn, ...args]);
  }
  function notifyWatches(value, oldState, state) {
    const method = builtin(value, "IWatchable/-notify-watches", 3);
    if (method) return method.call(value, value, oldState, state);
    if (!atoms.has(value)) return stateProtocol(value, "IWatchable/-notify-watches", [oldState, state]);
    // Hold the original persistent watch map through all synchronous callbacks.
    for (let s = seq(value.watches); s !== null; s = next(s)) {
      const entry = first(s);
      nth(entry, 1).call(null, nth(entry, 0), value, oldState, state);
    }
    return null;
  }
  function addWatchProtocol(value, key, fn) {
    const method = builtin(value, "IWatchable/-add-watch", 3);
    if (method) return method.call(value, value, key, fn);
    if (!atoms.has(value)) return stateProtocol(value, "IWatchable/-add-watch", [key, fn]);
    value.watches = assoc(value.watches, key, fn);
    return value;
  }
  function removeWatchProtocol(value, key) {
    const method = builtin(value, "IWatchable/-remove-watch", 2);
    if (method) return method.call(value, value, key);
    if (!atoms.has(value)) return stateProtocol(value, "IWatchable/-remove-watch", [key]);
    return value.watches = dissoc(value.watches, key);
  }
  function addWatch(value, key, fn) { addWatchProtocol(value, key, fn); return value; }
  function removeWatch(value, key) { removeWatchProtocol(value, key); return value; }
  function resetAtom(value, state, pair) {
    const validator = value.validator;
    if (validator != null && !truth(validator.call(null, state))) throw new Error("Validator rejected reference state");
    // A validator may have changed the atom; capture old state only afterward.
    const oldState = value.state;
    value.state = state;
    if (value.watches != null) notifyWatches(value, oldState, state);
    return pair ? vector(oldState, state) : state;
  }
  function reset(value, state) { return atoms.has(value) ? resetAtom(value, state, false) : resetProtocol(value, state); }
  function resetVals(value, state) { return atoms.has(value) ? resetAtom(value, state, true) : vector(deref(value), resetProtocol(value, state)); }
  function swapState(value, fn, args, pair, variadic = false) {
    if (atoms.has(value)) {
      const state = variadic ? apply(fn, value.state, first(args), first(next(args)), rest(next(args))) : fn.call(null, value.state, ...toArray(args));
      return resetAtom(value, state, pair);
    }
    const oldState = pair ? deref(value) : null;
    const inputs = variadic ? [fn, first(args), first(next(args)), rest(next(args))] : [fn, ...toArray(args)];
    const result = stateProtocol(value, "ISwap/-swap!", inputs);
    return pair ? vector(oldState, result) : result;
  }
  function swap(value, fn, ...args) {
    if (arguments.length < 2) return swapState(value, fn, cons(undefined, cons(undefined, null)), false, true);
    return swapState(value, fn, fnRest(args, 0), false, args.length > 2);
  }
  function swapVals(value, fn, ...args) {
    if (arguments.length < 2) return swapState(value, fn, cons(undefined, cons(undefined, null)), true, true);
    return swapState(value, fn, fnRest(args, 0), true, args.length > 2);
  }
  function compareAndSet(value, oldState, state) {
    // Both pinned CLJS runtimes use = here, including equal distinct collections.
    if (!equalPair(deref(value), oldState)) return false;
    reset(value, state);
    return true;
  }
  function setValidator(value, validator) {
    if (validator != null && !truth(validator.call(null, deref(value)))) throw new Error("Validator rejected reference state");
    return value.validator = validator;
  }
  function getValidator(value) { return value.validator; }
  function volatile(state) { return new Volatile(state); }
  function volatileFactory(state) { return new Volatile(state); }
  function isVolatile(value) { return volatiles.has(value); }
  function vreset(value, state) { return vresetProtocol(value, state); }
  function vresetProtocol(value, state) {
    const method = builtin(value, "IVolatile/-vreset!", 2);
    if (method) return method.call(value, value, state);
    if (volatiles.has(value)) return value.state = state;
    return stateProtocol(value, "IVolatile/-vreset!", [state]);
  }
  function update(value, key, fn, ...args) {
    const old = get(value, key);
    return assoc(value, key, arguments.length >= 3 && arguments.length <= 6
      ? fn.call(null, old, ...args)
      : apply(fn, old, args[0], args[1], args[2], fnRest(args, 3)));
  }
  function updatedMap(original, entries, removeRecord = false) {
    const Type = !removeRecord && recordTypes.get(original);
    if (mapKinds.get(original) === "sorted-map") {
      const prior = maps.get(original);
      if (entries.length === prior.length && entries.every((entry, i) => entry === prior[i])) return original;
      return preserveMeta(original, makeMap(entries, "sorted-map", sortedComparators.get(original)));
    }
    const result = Type ? new Type(...Type.fieldNames.map(name => {
      const entry = entries.find(e => equalPair(e.key, keyword(null, name)));
      return entry?.value;
    })) : makeMap(entries, mapKinds.get(original) === "hash-map" || (entries.length > 8 && entries.length > (maps.get(original)?.length ?? 0)) ? "hash-map" : "array-map");
    if (Type) { maps.set(result, Object.freeze(entries)); mapKinds.set(result, mapKinds.get(original)); }
    return preserveMeta(original, result);
  }
  function withMeta(value, metadata) {
    if (value == null) return null;
    const method = typeof value === "function" ? null : builtin(value, "IWithMeta/-with-meta", 2);
    if (method) return method.call(value, value, metadata);
    if (!recordTypes.has(value) && !mapEntries.has(value) &&
        [vectors, maps, sets, cells, views, queues, lazyValues].some(store => store.has(value)) && meta(value) === metadata) return value;
    let result;
    if (mapEntries.has(value)) return withMeta(vectorFromArray(vectors.get(value)), metadata);
    if (typeof value === "function" || metaFunctions.has(value)) {
      const original = metaFunctions.get(value) || value;
      const methods = {};
      for (let n = 0; n <= 20; n++) methods[n] = function (_self, ...args) { return original.call(null, ...args); };
      methods[21] = function (_self, ...args) { return apply(original, ...args); };
      result = makeIFn(methods);
      metaFunctions.set(result, original);
    }
    else if (lazyValues.has(value)) {
      result = new LazySeq(metadata, () => seq(value), null, lazyValues.get(value).hash);
    }
    else if (cells.get(value)?.kind === "chunked-cons") {
      const c = cells.get(value);
      result = new ChunkedCons(c.chunk, c.more, metadata, c.hash);
    }
    else if (cells.get(value)?.kind === "iterate") {
      const c = cells.get(value), state = c.state;
      result = iterateNode(c.fn, c.previous, state.value, state.realized, state.following);
    }
    else if (cells.get(value)?.kind === "cycle") {
      const c = cells.get(value), state = c.state;
      result = cycleNode(c.all, c.previous, state.current, state.following);
    }
    else if (recordTypes.has(value)) {
      // A record copy owns fresh mutable host fields. Copying accessor
      // descriptors would leave their closures pointing at the old instance.
      const Type = recordTypes.get(value);
      result = new Type(...Type.fieldNames.map(name => field(value, name)));
      maps.set(result, maps.get(value)); mapKinds.set(result, mapKinds.get(value));
    }
    else {
      const stores = [symbols, vectors, maps, mapKinds, setKinds, sortedComparators, sets, cells, views, queues, lazyValues, vars, subvecSources];
      if (!stores.some(store => store.has(value)) && !reifyValues.has(value)) throw new Error("value does not support metadata");
      result = Object.create(Object.getPrototypeOf(value));
      for (const store of [...stores, protocolImplementations, typeFields, recordTypes]) if (store.has(value)) store.set(result, store.get(value));
      if (reifyValues.has(value)) reifyValues.add(result);
      if (ifnValues.has(value)) ifnValues.set(result, ifnValues.get(value));
      if (ifnValues.has(value) || reifyValues.has(value) || recordTypes.has(value)) Object.defineProperties(result, Object.getOwnPropertyDescriptors(value));
    }
    metadataValues.set(result, metadata);
    return result;
  }
  const uuidPrototype = Object.freeze({toString() { return uuids.get(this); }});
  function uuid(text) {
    if (typeof text !== "string") throw new Error("UUID expects a string");
    return uuidValue(text.toLowerCase());
  }
  function uuidValue(text, cachedHash = null) {
    const value = builtinValue("UUID", uuidPrototype);
    uuids.set(value, text);
    if (cachedHash != null) uuidHashes.set(value, cachedHash);
    Object.defineProperty(value, "__hash", {get() { return uuidHashes.get(this) ?? null; }});
    return value;
  }
  // Explicit embedding hooks. Scheduling is optional; the default dispatches
  // synchronously, with a finite drain budget even when taps enqueue more taps.
  const tapListeners = new Set(), tapQueue = [];
  let tapDraining = false, tapDelivered = 0, tapPending = 0;
  let tapLimit = 1024;
  function defaultTapSchedule(task) { task.call(null); return true; }
  function configureHostHooks(options = {}) {
    for (const key of ["clock", "random", "tapSchedule"]) {
      if (Object.hasOwn(options, key) && options[key] !== null && typeof options[key] !== "function") throw new TypeError(key + " hook must be a function or null");
    }
    if (Object.hasOwn(options, "maxPendingTaps") && (!Number.isSafeInteger(options.maxPendingTaps) || options.maxPendingTaps < 1)) throw new TypeError("maxPendingTaps must be a positive integer");
    if (Object.hasOwn(options, "clock")) core.hostClock = options.clock ?? defaultHostClock;
    if (Object.hasOwn(options, "random")) core.hostRandom = options.random ?? defaultHostRandom;
    if (Object.hasOwn(options, "tapSchedule")) core.execTapFn = options.tapSchedule ?? defaultTapSchedule;
    if (Object.hasOwn(options, "maxPendingTaps")) tapLimit = options.maxPendingTaps;
    return null;
  }
  function defaultHostClock() {
    return typeof globalThis.performance?.now === "function" ? globalThis.performance.now() : Date.now();
  }
  function defaultHostRandom() { return Math.random(); }
  function systemTime() {
    if (arguments.length) return invalidArity(arguments.length);
    const value = core.hostClock.call(null);
    if (typeof value !== "number" || !Number.isFinite(value)) throw new TypeError("clock hook must return a finite number");
    return value;
  }
  function hostRandom() {
    const value = core.hostRandom.call(null);
    if (typeof value !== "number" || !(value >= 0 && value < 1)) throw new TypeError("random hook must return a number in [0, 1)");
    return value;
  }
  function hostRand(n) {
    if (arguments.length > 1) return invalidArity(arguments.length);
    return arguments.length ? hostRandom() * n : hostRandom();
  }
  function hostRandInt(n) { if (arguments.length !== 1) return invalidArity(arguments.length); return Math.floor(hostRandom() * n); }
  function hostRandNth(coll) { if (arguments.length !== 1) return invalidArity(arguments.length); return nth(coll, hostRandInt(count(coll))); }
  function hostShuffle(coll) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    const items = toArray(coll);
    for (let i = items.length - 1; i > 0; i--) { const j = hostRandInt(i + 1); [items[i], items[j]] = [items[j], items[i]]; }
    return vectorFromArray(items);
  }
  function hostRandomUuid() {
    if (arguments.length) return invalidArity(arguments.length);
    return uuid("xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, digit => { const value = hostRandInt(16); return (digit === "x" ? value : (value & 3) | 8).toString(16); }));
  }
  function addTap(listener) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    if (listener == null || typeof listener.call !== "function") throw new TypeError("add-tap requires a callable value");
    tapListeners.add(listener); return null;
  }
  function removeTap(listener) { if (arguments.length !== 1) return invalidArity(arguments.length); tapListeners.delete(listener); return null; }
  function deliverTap(listeners, value) {
    for (const listener of listeners) { try { listener.call(null, value); } catch (_) { /* One listener cannot suppress the others. */ } }
  }
  function tapGreater(value) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    if (tapPending + tapQueue.length + (tapDraining ? tapDelivered : 0) >= tapLimit) return false;
    const schedule = core.execTapFn;
    if (schedule !== defaultTapSchedule) {
      tapPending++;
      let active = true;
      const task = () => {
        if (!active) return null;
        active = false; tapPending--;
        // The same drain guard bounds a scheduler which chooses to run inline.
        tapQueue.push(value); drainTaps(); return null;
      };
      try {
        const admitted = truth(schedule.call(null, task));
        if (!admitted && active) { active = false; tapPending--; }
        return admitted;
      } catch (_) { if (active) { active = false; tapPending--; } return false; }
    }
    tapQueue.push(value); drainTaps(); return true;
  }
  function drainTaps() {
    if (tapDraining) return;
    tapDraining = true; tapDelivered = 0;
    try {
      while (tapQueue.length && tapDelivered < tapLimit) {
        const value = tapQueue.shift(); tapDelivered++;
        deliverTap([...tapListeners], value);
      }
    } finally { tapQueue.length = 0; tapDraining = false; tapDelivered = 0; }
  }
  function makeVar(get, symbol, metadata = null, deferred = false) {
    const methods = {};
    for (let n = 0; n <= 20; n++) methods[n] = function (_self, ...args) { return get.call(null).call(null, ...args); };
    methods[21] = function (_self, ...args) { return apply(get.call(null), ...args); };
    const value = makeIFn(methods);
    Object.setPrototypeOf(value, varPrototype);
    vars.set(value, Object.freeze({get, symbol, deferred, definitionMetadata: metadata}));
    metadataValues.set(value, metadata);
    return value;
  }
  function definitionVar(get, symbol, metadata = null, deferred = false) {
    const root = deferred ? null : get.call(null);
    const base = mapFromArray([keyword(null, "ns"), symbolCore(namespace(symbol)), keyword(null, "name"), symbolCore(name(symbol)),
      keyword(null, "doc"), null, keyword(null, "file"), null, keyword(null, "line"), null, keyword(null, "column"), null, keyword(null, "arglists"), listFromArray([])]);
    return makeVar(get, symbol, assoc(into(base, metadata), keyword(null, "ns"), symbolCore(namespace(symbol)), keyword(null, "name"), symbolCore(name(symbol)), keyword(null, "test"), truth(root) ? root.cljs$lang$test : null), deferred);
  }
  function Var(get, symbol, metadata) { const value = makeVar(get, symbol, metadata); metadataValues.set(value, metadata); return value; }
  function test(value) {
    const entry = vars.get(value), metadata = entry ? meta(value) : null;
    const fn = entry ? (entry.deferred && metadata === entry.definitionMetadata
      ? entry.get.call(null)?.cljs$lang$test : get(metadata, keyword(null, "test")))
      : value == null ? null : value.cljs$lang$test;
    if (truth(fn)) { fn.call(null); return keyword(null, "ok"); }
    return keyword(null, "no-test");
  }
  const varPrototype = Object.freeze({
    constructor: Var,
    isMacro() { return vars.get(this).get.call(null).cljs$lang$macro; },
    get val() { return vars.get(this).get; },
    get sym() { return vars.get(this).symbol; },
    get _meta() { return meta(this); },
    apply(_self, args) { return ifnApply(this, args); },
    call(_self, ...args) { return ifnPrototype.call.call(this, _self, ...args); },
    toString() { return "#'" + vars.get(this).symbol; }
  });
  Var.prototype = varPrototype;
  // Namespace identities are session-owned, with source registration from emitted units.
  const namespaces = new Map();
  function Namespace(obj, name) {
    if (!(this instanceof Namespace)) return new Namespace(obj, name);
    this.obj = obj; this.name = name;
    installMethods(this, [["IEquiv/-equiv", 2, (_self, other) => other instanceof Namespace && equalPair(name, other.name)],
      ["IHash/-hash", 1, () => hash(name)]]);
  }
  Namespace.prototype.getName = function () { return this.name; };
  Namespace.prototype.toString = function () { return String(this.name); };
  Namespace.prototype.findInternedVar = function (local) {
    if (this.obj == null) return null;
    const key = mungeStr(str(local));
    if (!Object.prototype.hasOwnProperty.call(this.obj, key)) return null;
    return definitionVar(() => this.obj[key], symbol(str(this.name), str(local)), null, true);
  };
  function createNs(name, obj = null) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    if (!symbols.has(name)) throw new TypeError("create-ns requires a symbol");
    const key = str(name);
    if (!namespaces.has(key)) namespaces.set(key, new Namespace(obj, name));
    else if (obj != null) namespaces.get(key).obj = obj;
    return namespaces.get(key);
  }
  function findNs(name) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    if (name == null) return null;
    if (name instanceof Namespace) return name;
    if (!symbols.has(name)) throw new TypeError("find-ns requires a symbol or Namespace");
    return namespaces.get(str(name)) ?? null;
  }
  function nsName(value) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    // Definition metadata historically stores the source symbol; it is an identity view.
    if (symbols.has(value)) return value;
    if (!(value instanceof Namespace)) throw new TypeError("ns-name requires a Namespace");
    return value.name;
  }
  function theNs(name) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    const value = findNs(name);
    if (!value) throw new Error("No namespace: " + str(name));
    return value;
  }
  function allNs() { if (arguments.length) return invalidArity(arguments.length); return listFromArray([...namespaces.values()]); }
  function compilerMacroVar(sym, metadata) {
    if (arguments.length !== 2) return invalidArity(arguments.length);
    const base = mapFromArray([keyword(null, "ns"), symbolCore(namespace(sym)), keyword(null, "name"), symbolCore(name(sym)), keyword(null, "macro"), true]);
    return makeVar(() => { throw new Error("compile-time macro cannot be consumed as a runtime value: " + str(sym)); }, sym, into(base, metadata), true);
  }
  function rePattern(pattern) {
    if (pattern instanceof RegExp) return pattern;
    if (typeof pattern !== "string") throw new TypeError("regex pattern requires a string");
    const prefix = /^\(\?([dgimsuxy]*)\)/.exec(pattern);
    return new RegExp(prefix ? pattern.slice(prefix[0].length) : pattern, prefix ? prefix[1] : "");
  }
  function reMatch(re, text, full) {
    if (typeof text !== "string") throw new TypeError("regex matching requires a string");
    const result = re.exec(text);
    if (result === null || (full && result[0] !== text)) return null;
    return result.length === 1 ? result[0] : vectorFromArray(Array.from(result));
  }
  function deferredValue(state, lazy = false) {
    // Assignment occurs only after success; failures leave the thunk installed.
    // Reentrant calls are ordinary calls, matching CLJS's observable behavior.
    if (lazy ? state.thunk != null : truth(state.thunk)) { state.value = state.thunk.call(null); state.thunk = null; }
    return state.value;
  }
  function Delay(thunk, result) {
    const value = builtinValue("Delay", Object.prototype);
    delays.set(value, {thunk, value: result});
    return value;
  }
  function LazySeq(metadata, thunk, result, cachedHash) {
    const value = builtinValue("LazySeq", sequencePrototype);
    lazyValues.set(value, {thunk, value: result, hash: cachedHash});
    metadataValues.set(value, metadata);
    return value;
  }
  function lazySequence(thunk) { return new LazySeq(null, thunk, null, null); }
  function force(value) { return delays.has(value) ? deref(value) : value; }
  function nthrest(value, n) {
    while (n > 0) { const current = seq(value); if (current == null) break; value = rest(current); n--; }
    return value;
  }
  function nthnext(value, n) {
    value = seq(value);
    while (n > 0 && value != null) { value = next(value); n--; }
    return value;
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
    cells.set(result, Object.freeze({kind: "cons", head: value, tail}));
    return result;
  }
  function toArray(value) {
    const result = [];
    for (let current = seq(value); current !== null; current = next(current)) result.push(first(current));
    return result;
  }
  function vec(value) {
    if (predicates.isMapEntry(value)) return vector(core.keyProtocol(value), core.valProtocol(value));
    if (predicates.isVector(value)) return withMeta(value, null);
    return into(vector(), value);
  }
  function concatTwo(x, y) {
    return lazySequence(() => {
      const s = seq(x);
      if (s === null) return y;
      return implementsProtocol(IChunkedSeq, s) ? chunkCons(chunkFirst(s), concatTwo(chunkRest(s), y)) : cons(first(s), concatTwo(rest(s), y));
    });
  }
  function concatMany(current, remaining) {
    return lazySequence(() => {
      const s = seq(current);
      if (s !== null) return implementsProtocol(IChunkedSeq, s) ? chunkCons(chunkFirst(s), concatMany(chunkRest(s), remaining)) : cons(first(s), concatMany(rest(s), remaining));
      return remaining == null ? null : concatMany(first(remaining), next(remaining));
    });
  }
  function treeSeq(branch, children, root) {
    const walk = node => lazySequence(() => cons(node,
      truth(branch.call(null, node)) ? mapcat(walk, children.call(null, node)) : null));
    return walk(root);
  }
  function flatten(value) { return filter(x => !predicates.isSequential(x), rest(treeSeq(predicates.isSequential, seq, value))); }
  const concat = callable(2, {0: () => lazySequence(() => null), 1: x => lazySequence(() => x), 2: concatTwo}, (x, y, more) => concatMany(concatTwo(x, y), more));
  function es6Iterator(value) {
    let current = seq(value);
    return {next() {
      if (current === null) return {value: null, done: true};
      const value = first(current);
      current = next(current);
      return {value, done: false};
    }};
  }
  function es6EntriesIterator(value, setEntries = false) {
    const iterator = es6Iterator(value);
    return {next() {
      const item = iterator.next();
      if (!item.done) item.value = setEntries ? [item.value, item.value] : [nth(item.value, 0), nth(item.value, 1)];
      return item;
    }};
  }
  function sequenceIterator(value) {
    if (satisfies(IIterable, value)) return core.iteratorProtocol(value);
    if (value == null) return {hasNext() { return false; }, next() { return new Error("No such element"); }, remove() { return new Error("Unsupported operation"); }};
    if (typeof value === "string" || Array.isArray(value)) {
      let index = 0;
      return {hasNext() { return index < value.length; }, next() { return typeof value === "string" ? value.charAt(index++) : value[index++]; }, remove() { return new Error("Unsupported operation"); }};
    }
    if (predicates.isSeqable(value)) return new SeqIter(value);
    throw new Error("Cannot create iterator from " + str(value));
  }
  function transformedIterator(xform, sources) {
    let buffer = [], offset = 0, ready = false, item, completed = false;
    const sink = function (acc, value) {
      switch (arguments.length) {
        case 0: return null;
        case 1: return acc;
        case 2: buffer.push(value); return acc;
        default: return invalidArity(arguments.length);
      }
    };
    const xf = xform.call(null, sink);
    return {
      hasNext() {
        if (ready) return true;
        while (offset === buffer.length) {
          buffer = []; offset = 0;
          if (completed) return false;
          // MultiIterator probes sources from left to right and stops at the
          // shortest. Request next values only after every probe succeeds.
          if (sources.every(source => source.hasNext())) {
            const result = sources.length === 1
              ? xf.call(null, null, sources[0].next())
              : xf.call(null, null, ...sources.map(source => source.next()));
            if (!isReduced(result)) continue;
          }
          xf.call(null, null);
          completed = true;
        }
        item = buffer[offset]; buffer[offset++] = null; ready = true;
        return true;
      },
      next() {
        if (!this.hasNext()) throw new Error("No such element");
        const value = item; item = null; ready = false;
        return value;
      }
    };
  }
  function chunkIteratorSequence(iter) {
    return lazySequence(() => {
      if (!iter.hasNext()) return null;
      const values = [];
      // The CLJS condition probes the iterator BEFORE checking the chunk
      // size, caching one extra output for the following chunk.
      while (iter.hasNext() && values.length < 32) values.push(iter.next());
      return chunkCons(arrayChunk(values), chunkIteratorSequence(iter));
    });
  }
  const sequence = callable(2, {
    1: value => predicates.isSeq(value) ? value : seq(value) ?? emptyList,
    2: (xform, value) => chunkIteratorSequence(transformedIterator(xform, [sequenceIterator(value)]))
  }, (xform, value, more) => chunkIteratorSequence(transformedIterator(xform, [value, ...toArray(more)].map(sequenceIterator))));
  function Eduction(xform, coll) {
    // Retain only the view's inputs; each protocol call owns its traversal state.
    // This is not a collection payload with structural equality or metadata.
    const value = builtinValue("Eduction", Object.prototype);
    eductions.set(value, {xform, coll});
    return value;
  }
  const eduction = callable(0, {}, args => {
    const values = toArray(args);
    return new Eduction(comp(...values.slice(0, -1)), values.length ? values.at(-1) : null);
  });
  function fnRest(args, start) {
    return args.length > start ? view(Object.freeze(Array.prototype.slice.call(args, start)), 0, true) : null;
  }
  function fnApplyTo(delegate, fixed, args) {
    const prefix = [];
    // The final rest preserves sequence identity and laziness. For a positive
    // prefix, CLJS uses rest after the last fixed argument, not next.
    for (let i = 0; i < fixed; i++) {
      prefix.push(first(args));
      args = i + 1 === fixed ? rest(args) : next(args);
    }
    return delegate.call(null, ...prefix, fixed === 0 ? seq(args) : args);
  }
  // This is the callable object ABI used by IFn and metadata wrappers.
  // The last method receives twenty values followed by a sequence tail.
  function makeIFn(methods, invokeMethods = methods) {
    const value = Object.create(ifnPrototype);
    ifnValues.set(value, methods);
    for (const [arity, method] of Object.entries(invokeMethods)) {
      value["cljs$core$IFn$_invoke$arity$" + arity] = function (...args) {
        return method.call(this, this, ...args);
      };
    }
    reifyValues.add(value);
    return value;
  }
  const protocolKeys = new WeakMap(), protocolImplementations = new WeakMap();
  const protocolNames = new WeakMap(), protocolValues = new Map(), protocolFunctions = new Map(), protocolVariables = new Map(), protocolGetters = new Map();
  const nativeMethods = new Map(), coreDispatchers = new Map();
  function protocol(name) {
    const value = function () {};
    protocolNames.set(value, name); setDeclaration(protocolValues, name, value);
    return value;
  }
  function nativeType(value) { return value == null ? "null" : Array.isArray(value) ? "array" : typeof value; }
  const protocolSentinel = Object.freeze({}), protocolSlots = new Map();
  function protocolSlot(key, arity) {
    let slots = protocolSlots.get(key);
    if (slots?.has(arity)) return slots.get(arity);
    if (!slots) { slots = new Map(); protocolSlots.set(key, slots); }
    const slash = key.indexOf("/");
    const qualified = arity === 0 ? slash >= 0 : key.indexOf("/", slash + 1) >= 0;
    const namespace = qualified ? key.slice(0, slash) : "cljs.core";
    const remainder = qualified ? key.slice(slash + 1) : key;
    const separator = remainder.startsWith("//") ? 1 : remainder.indexOf("/");
    const name = arity === 0 ? remainder : remainder.slice(0, separator);
    const method = remainder.slice(separator + 1);
    const prefix = mungeMember(namespace).replaceAll(".", "$") + "$" + mungeMember(name) + "$";
    const slot = arity === 0 ? prefix : prefix + (method === "/" ? "_SLASH_" : mungeMember(method)) + "$arity$" + arity;
    slots.set(arity, slot); return slot;
  }
  function directMethod(value, key, arity) {
    demandProtocol(arity === 0 ? key : key.slice(0, key.lastIndexOf("/")), value);
    if (value == null) return undefined;
    const method = value[protocolSlot(key, arity)];
    return arity === 0 ? (method === protocolSentinel ? method : undefined) : method;
  }
  const protocolStores = [multiFns, transients, keywords, symbols, vectors, maps, sets, cells, views, queues, lazyValues, delays, vars, atoms, volatiles, reducedValues, metaFunctions];
  function ownedProtocolValue(value) { return protocolStores.some(store => store.has(value)); }
  function builtin(value, key, arity) {
    const method = directMethod(value, key, arity);
    if (method != null) return method;
    const external = coreDispatchers.get(key);
    const dispatch = external?.[nativeType(value)] ?? external?._;
    if (!nativeMethods.size && !dispatch) return undefined;
    const name = key.split("/")[0];
    const protocol = coreProtocolsByName.get(name);
    // Native-object fallback cannot override an owned collection's direct
    // core implementation. Its public operation handles the private payload.
    if (protocol && builtinCoreImplementation(protocol, value)) return undefined;
    return dispatch ?? nativeMethods.get(key)?.get(nativeType(value))?.get(arity) ?? nativeMethods.get(key)?.get("_")?.get(arity);
  }
  // Public consumers test protocol membership before selecting an arity. A
  // declared but omitted method must not silently fall back to another API.
  function publicMethod(value, key, arity, fast = false, directOnly = false) {
    const protocol = coreProtocolsByName.get(key.split("/")[0]);
    const direct = implementsProtocol(protocol, value);
    if (!direct && (directOnly || (fast && (Array.isArray(value) || typeof value === "string")))) return undefined;
    if (!direct && !satisfies(protocol, value)) return undefined;
    const method = builtin(value, key, arity);
    if (method) return method;
    if (protocol === IMeta && typeof value === "function") return undefined;
    if (satisfies(protocol, value) && !builtinCoreImplementation(protocol, value))
      throw new Error("No protocol method " + key + " defined for this value");
    return undefined;
  }
  function protocolMethod(key, metadataName, arities, viaMetadata = false, variadic = null, getter = null) {
    const dispatch = function (receiver, ...args) {
      const fixed = arities?.filter(n => n !== (variadic == null ? -1 : variadic + 2));
      const variadicCall = variadic != null && !fixed.includes(arguments.length);
      const arity = variadicCall ? variadic + 2 : variadic == null && arities?.length === 1 ? arities[0] : arguments.length;
      if (!variadicCall && fixed?.length > 1 && !fixed.includes(arity)) return invalidArity(arguments.length);
      if (variadicCall) {
        // CLJS accepts these declarations with a warning. Its macro's literal
        // signature expansion forwards the unbound ampersand and packed tail.
        const actual = [receiver, ...args];
        args = [...Array.from({length: variadic - 1}, (_, i) => actual[i + 1]), undefined, fnRest(actual, variadic)];
      } else if (variadic == null && arities?.length === 1) args = Array.from({length: arity - 1}, (_, i) => args[i]);
      if (viaMetadata) {
        const implementation = get(meta(receiver), symbolCore(metadataName));
        if (truth(implementation)) return implementation.call(null, receiver, ...args);
      }
      const method = directMethod(receiver, key, arity);
      if (method != null) return method.call(receiver, receiver, ...args);
      // Saved method values retain their direct slot while dynamic dispatch
      // observes the current method Var, as the CLJS macro expansion does.
      const current = getter ? getter() : protocolVariables.get(metadataName) ?? dispatch;
      const external = current[nativeType(receiver)] ?? current._;
      if (external != null) return external.call(null, receiver, ...args);
      throw new Error("No protocol method " + metadataName + " defined for this value");
    };
    protocolKeys.set(dispatch, key); setDeclaration(protocolFunctions, key, metadataName); setDeclaration(protocolVariables, metadataName, dispatch);
    setDeclaration(protocolGetters, key, getter ?? (() => protocolVariables.get(metadataName)));
    return dispatch;
  }
  function extendType(target, methods) {
    let result = null;
    if (typeof target === "string") {
      const grouped = new Map();
      for (const [key, arity, method] of methods) {
        if (arity === 0) {
          const value = protocolValues.get(key) ?? coreProtocolsByName.get(key);
          if (value) { patchDeclaration(value, target, {value: true, writable: true, configurable: true, enumerable: true}); result = true; }
        }
        if (!grouped.has(key)) grouped.set(key, new Map());
        grouped.get(key).set(arity, method);
      }
      for (const [key, implementations] of grouped) {
        if (implementations.has(0)) continue;
        const dispatch = protocolGetters.get(key)?.() ?? coreDispatchers.get(key);
        const implementation = function (...args) {
          const method = implementations.size === 1 ? implementations.values().next().value : implementations.get(args.length);
          return method ? method.call(null, ...args) : invalidArity(args.length);
        };
        result = implementation;
        if (dispatch) patchDeclaration(dispatch, target, {value: implementation, writable: true, configurable: true, enumerable: true});
        if (!protocolFunctions.has(key)) {
          if (!nativeMethods.has(key)) setDeclaration(nativeMethods, key, new Map());
          setDeclaration(nativeMethods.get(key), target, implementations);
        }
      }
    } else {
      if (typeof target !== "function" || target.prototype == null) throw new TypeError("extend-type requires a type constructor");
      installMethods(target.prototype, methods);
      result = methods.length ? (methods.at(-1)[1] === 0 ? protocolSentinel : methods.at(-1)[2]) : null;
    }
    return result;
  }
  function extendInstance(target, methods, returnTarget) {
    installMethods(target, methods);
    return returnTarget ? target : methods.length ? (methods.at(-1)[1] === 0 ? protocolSentinel : methods.at(-1)[2]) : null;
  }
  function installMethods(value, methods) {
    if (value != null && typeof value !== "object" && typeof value !== "function") return value;
    const oldImplementations = protocolImplementations.get(value), oldCalls = ifnValues.get(value);
    const transaction = declarationTransactions.at(-1);
    const implementations = new Map(Array.from(oldImplementations ?? [], ([key, entries]) => [key, new Map(entries)])), calls = {}, invokes = {}, objectVariadics = new Map();
    if (transaction) transaction.push({
      undo: () => { if (oldImplementations) protocolImplementations.set(value, oldImplementations); else protocolImplementations.delete(value); if (oldCalls) ifnValues.set(value, oldCalls); else ifnValues.delete(value); },
      redo: () => { protocolImplementations.set(value, implementations); if (Object.keys(calls).length) ifnValues.set(value, calls); },
      snapshot: () => { const methods = protocolImplementations.get(value), functions = ifnValues.get(value); return () => { if (methods) protocolImplementations.set(value, methods); else protocolImplementations.delete(value); if (functions) ifnValues.set(value, functions); else ifnValues.delete(value); }; }
    });
    for (const [dispatch, arity, method, invoke = method, variadic] of methods) {
      const key = typeof dispatch === "string" ? dispatch : protocolKeys.get(dispatch);
      if (!implementations.has(key)) implementations.set(key, new Map());
      if (variadic != null && key.startsWith("Object/")) objectVariadics.set(key, [variadic, method]);
      else implementations.get(key).set(arity, method);
      if (!key.startsWith("Object/") && key !== "IFn/-invoke") patchDeclaration(value, protocolSlot(key, arity), {value: arity === 0 ? protocolSentinel : method, writable: true, configurable: true});
      if (key === "IFn/-invoke") { calls[arity - 1] = method; invokes[arity - 1] = invoke; }
    }
    for (const [key, entries] of implementations) if (key.startsWith("Object/")) {
      patchDeclaration(value, mungeMember(key.slice(7)), {value: function (...args) {
        const rest = objectVariadics.get(key);
        const method = !rest && entries.size === 1 ? entries.values().next().value : entries.get(args.length + 1) ?? rest?.[1];
        if (!method) return invalidArity(args.length);
        // The pinned CLJS multi-arity Object dispatcher calls the variadic
        // entry as a property of its delegate function, losing the instance.
        const receiver = rest && entries.size && !entries.has(args.length + 1) ? method : this;
        return method.call(receiver, receiver, ...args);
      }, writable: true, configurable: true});
    }
    if (Object.keys(calls).length) {
      ifnValues.set(value, calls);
      patchDeclaration(value, "call", {value: ifnPrototype.call, writable: true, configurable: true});
      patchDeclaration(value, "apply", {value: ifnPrototype.apply, writable: true, configurable: true});
      for (const [arity, method] of Object.entries(invokes)) patchDeclaration(value, "cljs$core$IFn$_invoke$arity$" + arity, {value: function (...args) { return method.call(this, this, ...args); }, writable: true, configurable: true, enumerable: true});
    }
    protocolImplementations.set(value, implementations);
    return value;
  }
  function makeReify(methods) { const value = installMethods({}, methods); reifyValues.add(value); return value; }
  const reifySites = new Map();
  function reify(site, fields, values, methods, metadata = null) {
    let Type = reifySites.get(site);
    if (!Type) {
      Type = makeType(fields, [
        ["IMeta", 0, null], ["IWithMeta", 0, null],
        ["IMeta/-meta", 1, function () { return metadataValues.has(this) ? metadataValues.get(this) : null; }],
        ["IWithMeta/-with-meta", 2, function (_self, metadata) {
          const result = new Type(...fields.map(name => field(this, name)));
          reifyValues.add(result);
          metadataValues.set(result, metadata);
          return result;
        }], ...methods()], false);
      reifySites.set(site, Type);
    }
    const value = new Type(...values);
    metadataValues.set(value, metadata);
    reifyValues.add(value);
    return value;
  }
  function field(value, name) { return value[mungeMember(name)]; }
  const memberMunging = {"-":"_",":":"_COLON_","+":"_PLUS_",">":"_GT_","<":"_LT_","=":"_EQ_","~":"_TILDE_","!":"_BANG_","@":"_CIRCA_","#":"_SHARP_","'":"_SINGLEQUOTE_","\"":"_DOUBLEQUOTE_","%":"_PERCENT_","^":"_CARET_","&":"_AMPERSAND_","*":"_STAR_","|":"_BAR_","{":"_LBRACE_","}":"_RBRACE_","[":"_LBRACK_","]":"_RBRACK_","/":"$","\\":"_BSLASH_","?":"_QMARK_"};
  function mungeMember(name) { return Array.from(name, c => memberMunging[c] ?? c).join(""); }
  const mungeCharacters = {...memberMunging, "/": "_SLASH_"};
  // These two upstream CHAR_MAP entries are two-character strings, while the
  // public function iterates one UTF-16 character at a time.
  delete mungeCharacters['"']; delete mungeCharacters["\\"];
  mungeCharacters['\\"'] = "_DOUBLEQUOTE_"; mungeCharacters["\\\\"] = "_BSLASH_";
  const demungeCharacters = Object.fromEntries(Object.entries(mungeCharacters).map(([a, b]) => [b, a]));
  const demungePattern = new RegExp(Object.keys(demungeCharacters).sort((a, b) => b.length - a.length).join("|") + "|\\$", "g");
  const reservedNames = new Set("arguments abstract await boolean break byte case catch char class const continue debugger default delete do double else enum export extends final finally float for function goto if implements import in instanceof int interface let long native new package private protected public return short static super switch synchronized this throw throws transient try typeof var void volatile while with yield methods null constructor".split(" "));
  function mungeStr(value) { let result = ""; for (let i = 0; i < value.length; i++) result += mungeCharacters[value.charAt(i)] ?? value.charAt(i); return result; }
  function munge(value) { let result = mungeStr(str(value)); result = result === ".." ? "_DOT__DOT_" : reservedNames.has(result) ? result + "$" : result; return symbols.has(value) ? symbolCore(result) : result; }
  function demunge(value) { let result = str(value); result = result === "_DOT__DOT_" ? ".." : result.replace(/\$$/, "").replace(demungePattern, key => key === "$" ? "/" : demungeCharacters[key]); return symbols.has(value) ? symbolCore(result) : result; }
  // Surface records supply protocol methods just as the upstream macro does.
  // Raw defrecord* only emits a constructor; its explicit body installs methods.
  function mapEntry(key, value) { const pair = vector(key, value); Object.setPrototypeOf(pair, builtinType("MapEntry", vectorPrototype).prototype); mapEntries.add(pair); return pair; }
  function recordEntries(value, Type) {
    return [...Type.fieldNames.map(name => mapEntry(keyword(null, name), field(value, name))), ...toArray(value.__extmap)];
  }
  function recordCopy(value, Type, metadata, extmap, cachedHash) {
    const Constructor = Type.currentConstructor?.() ?? Type;
    return new Constructor(...Type.fieldNames.map(name => field(value, name)), metadata, extmap, cachedHash);
  }
  function recordMethods(Type) {
    const copy = (value, metadata, extmap, cachedHash) => recordCopy(value, Type, metadata, extmap, cachedHash);
    const fieldIndex = key => Type.fieldNames.findIndex(name => equalPair(key, keyword(null, name)));
    const methods = [
      ["ICloneable/-clone", 1, function () { return recordCopy(this, Type, this.__meta, this.__extmap, this.__hash); }],
      ["IHash/-hash", 1, function () {
        if (this.__hash != null) return this.__hash;
        let result = 0;
        for (const entry of recordEntries(this, Type)) result = (result + (hash(nth(entry, 0)) ^ hash(nth(entry, 1)))) | 0;
        return this.__hash = result ^ stringHash(typeNames.get(Type) ?? "");
      }],
      ["IEquiv/-equiv", 2, function (_self, other) {
        return other != null && this.constructor === other.constructor &&
          Type.fieldNames.every(name => equalPair(field(this, name), field(other, name))) && equalPair(this.__extmap, other.__extmap);
      }],
      ["IMeta/-meta", 1, function () { return this.__meta; }],
      ["IWithMeta/-with-meta", 2, function (_self, metadata) { return copy(this, metadata, this.__extmap, this.__hash); }],
      ["ILookup/-lookup", 2, function (_self, key) { return get(this, key, null); }],
      ["ILookup/-lookup", 3, function (_self, key, fallback) {
        const index = fieldIndex(key); return index < 0 ? get(this.__extmap, key, fallback) : field(this, Type.fieldNames[index]);
      }],
      ["ICounted/-count", 1, function () { return Type.fieldNames.length + count(this.__extmap); }],
      ["ICollection/-conj", 2, function (_self, entry) {
        if (predicates.isVector(entry)) return assoc(this, nth(entry, 0), nth(entry, 1));
        return reduce((result, pair) => conj(result, pair), this, entry);
      }],
      ["IAssociative/-contains-key?", 2, function (_self, key) { return fieldIndex(key) >= 0 || contains(this.__extmap, key); }],
      ["IAssociative/-assoc", 3, function (_self, key, value) {
        const index = fieldIndex(key);
        if (index < 0) return copy(this, this.__meta, assoc(this.__extmap, key, value), null);
        const Constructor = Type.currentConstructor?.() ?? Type;
        return new Constructor(...Type.fieldNames.map((name, i) => i === index ? value : field(this, name)), this.__meta, this.__extmap, null);
      }],
      ["IMap/-dissoc", 2, function (_self, key) {
        if (fieldIndex(key) >= 0) return dissoc(withMeta(into(mapFromArray([]), this), this.__meta), key);
        const extmap = dissoc(this.__extmap, key);
        return copy(this, this.__meta, seq(extmap) === null ? null : extmap, null);
      }],
      ["ISeqable/-seq", 1, function () { return seq(vectorFromArray(recordEntries(this, Type))); }],
      ["IIterable/-iterator", 1, function () { return new SeqIter(this); }],
      ["IPrintWithWriter/-pr-writer", 3, function (_self, writer, options) {
        return prSequentialWriter(writer, (entry, target, opts) => prSequentialWriter(target, printWriter, "", " ", "", opts, entry),
          "#" + (typeNames.get(Type) ?? "").replaceAll("/", ".") + "{", ", ", "}", options, recordEntries(this, Type));
      }],
      ["IKVReduce/-kv-reduce", 3, function (_self, fn, init) {
        return reduce((result, entry) => fn.call(null, result, nth(entry, 0), nth(entry, 1)), init, this);
      }]
    ];
    return [["IRecord", 0, null], ...Array.from(new Set(methods.map(([key]) => key.split("/")[0])), key => [key, 0, null]), ...methods];
  }
  function makeType(fields, methods, record, masks = null, name = null, currentConstructor = null) {
    let implicitMasks;
    const allFields = record ? [...fields, "__meta", "__extmap", "__hash"] : fields;
    const Type = function (...args) {
      const value = this, data = Object.create(null);
      for (let i = 0; i < allFields.length; i++) {
        const name = allFields[i]; data[name] = args[i];
        Object.defineProperty(value, mungeMember(name), {enumerable: true, configurable: true,
          get() { return data[name]; },
          set(state) { data[name] = state; }});
      }
      if (masks != null) for (const entry of maps.get(masks) ?? []) value["cljs$lang$protocol_mask$partition" + entry.key + "$"] = entry.value;
      else for (let i = 0; i < implicitMasks.length; i++) value["cljs$lang$protocol_mask$partition" + i + "$"] = implicitMasks[i];
      typeFields.set(value, data);
      if (ifnValues.has(Type.prototype)) ifnValues.set(value, ifnValues.get(Type.prototype));
      if (record === true) { recordTypes.set(value, Type); mapKinds.set(value, "record"); maps.set(value, Object.freeze([])); }
    };
    Type.fieldNames = fields;
    if (currentConstructor) Type.currentConstructor = currentConstructor;
    if (name != null) typeNames.set(Type, name);
    const implementations = record === true ? [...methods, ...recordMethods(Type)] : methods;
    implicitMasks = [0, 0];
    for (const [key, arity] of implementations) {
      const index = arity === 0 ? protocolMaskNames.lastIndexOf(key) : -1;
      if (index >= 0) implicitMasks[index >> 5] |= 1 << (index % 32);
    }
    installMethods(Type.prototype, implementations);
    return Type;
  }
  function recordFromMap(Type, map) {
    let extmap = dissoc(map, ...Type.fieldNames.map(name => keyword(null, name)));
    if (predicates.isRecord(map)) extmap = into(mapFromArray([]), extmap);
    return new Type(...Type.fieldNames.map(name => get(map, keyword(null, name))), null, seq(extmap) === null ? null : extmap, null);
  }
  function ifnMethods(value) {
    return ifnValues.get(value) ?? (value == null ? undefined : ifnValues.get(Object.getPrototypeOf(value)));
  }
  const ifnPrototype = Object.freeze({
    call(_self, ...args) {
      const methods = ifnMethods(this), keys = Object.keys(methods);
      const method = keys.length === 1 ? methods[keys[0]] : methods[args.length];
      if (!method) return invalidArity(args.length);
      return method.call(this, this, ...args);
    },
    apply(_self, args) { return ifnApply(this, args); }
  });
  // Unlike Function.apply, an IFn apply must bundle the 21st and later values
  // exactly once. Keep that path independent of .call's already-expanded ABI.
  function ifnApply(value, args) {
    const values = Array.from(args);
    return value.call(null, ...values.slice(0, 20), ...(values.length > 20 ? [values.slice(20)] : []));
  }
  function boundedCount(n, value) {
    if (predicates.isCounted(value)) return count(value);
    let total = 0;
    for (let current = seq(value); n > 0 && current !== null; n--, current = next(current)) total++;
    return total;
  }
  function applyTo(fn, n, args) {
    args = seq(args);
    if (!Number.isInteger(n) || n < 0 || n > 20) throw new Error("Only up to 20 arguments supported on functions");
    const values = [];
    for (let i = 0; i < n; i++, args = rest(args)) values.push(first(args));
    return fn.call(null, ...values);
  }
  function apply(fn, ...args) {
    // The upstream variadic default also receives calls with fewer than two
    // arguments. Keep its missing leading slots rather than adding validation.
    if (args.length === 0) args = [undefined, undefined, undefined, undefined, null];
    const hasApplyTo = fn.cljs$lang$applyTo;
    let tail = args.pop();
    const prefix = args;
    if (prefix.length >= 4) tail = seq(tail);
    if (hasApplyTo) {
      let arglist = tail;
      for (let i = prefix.length - 1; i >= 0; i--) arglist = cons(prefix[i], arglist);
      const fixed = fn.cljs$lang$maxFixedArity;
      const n = prefix.length + boundedCount(fixed + 1 - prefix.length, tail);
      return n <= fixed ? applyTo(fn, n, arglist) : fn.cljs$lang$applyTo(arglist);
    }
    const values = prefix.concat(toArray(tail));
    if (values.length > 20) return fn.apply(fn, values);
    const entry = fn["cljs$core$IFn$_invoke$arity$" + values.length];
    return entry ? entry.call(fn, ...values) : fn.call(fn, ...values);
  }
  function listStar(...args) {
    if (args.length === 0) args = [undefined, undefined, undefined, undefined, null];
    let result = args.pop();
    if (args.length === 0) return seq(result);
    if (args.length >= 4) result = seq(result);
    for (let i = args.length - 1; i >= 0; i--) result = cons(args[i], result);
    return result;
  }
  function iterateNode(fn, previous, value, realized, following = null) {
    const result = builtinValue("Iterate", sequencePrototype), state = {value, realized, following};
    cells.set(result, Object.freeze({kind: "iterate", fn, previous, state,
      get realized() { return state.realized; },
      get head() { if (!state.realized) { state.value = fn.call(null, previous); state.realized = true; } return state.value; },
      get tail() { return state.following ?? (state.following = iterateNode(fn, first(result), undefined, false)); }}));
    return result;
  }
  function iterate(fn, seed) { return iterateNode(fn, null, seed, true); }
  // Chunk values expose indexed/count/reduce protocols, without being seqs.
  const chunks = new WeakMap(), chunkBuffers = new WeakMap();
  function ChunkBuffer(array, end) {
    const value = builtinValue("ChunkBuffer", Object.prototype);
    chunkBuffers.set(value, {array, end});
    return value;
  }
  function chunkBuffer(capacity) { return new ChunkBuffer(new Array(capacity), 0); }
  function chunkAppend(buffer, item) { const state = chunkBuffers.get(buffer); state.array[state.end] = item; return ++state.end; }
  function chunk(buffer) { const state = chunkBuffers.get(buffer), value = arrayChunk(state.array, 0, state.end); state.array = null; return value; }
  function arrayChunk(array, offset = 0, end = array.length) {
    const result = builtinValue("ArrayChunk", Object.prototype);
    chunks.set(result, {array, offset, end});
    return result;
  }
  function chunkNth(chunk, i, fallback = missing) {
    const state = chunks.get(chunk), size = state.end - state.offset;
    if (state.integer && fallback === missing) return state.start + i * state.step;
    if (i >= 0 && i < size) return state.integer ? state.start + i * state.step : state.array[state.offset + i];
    if (fallback !== missing) return fallback;
    // ArrayChunk's two-argument -nth reads the backing array directly.
    return state.array?.[state.offset + i];
  }
  function chunkFirst(value) { return core.chunkedFirstProtocol(value); }
  function chunkRest(value) { return core.chunkedRestProtocol(value); }
  function chunkNext(value) {
    return implementsProtocol(IChunkedNext, value) ? core.chunkedNextProtocol(value) : seq(chunkRest(value));
  }
  function ownedChunkFirst(value) {
    const cell = cells.get(value);
    if (cell?.kind === "chunked-cons") return cell.chunk;
    if (cell?.kind === "range") {
      if (cell.integer) {
        const chunk = arrayChunk([], 0, Math.min(32, cell.count));
        chunks.set(chunk, {...chunks.get(chunk), integer: true, start: cell.head, step: cell.step});
        return chunk;
      }
      return forceRangeChunk(cell).chunk;
    }
    const slice = views.get(value);
    const end = Math.min(slice.storage.length, slice.offset + 32 - slice.offset % 32);
    return arrayChunk(slice.storage, slice.offset, end);
  }
  const rangeChunks = new WeakMap();
  function forceRangeChunk(cell) {
    if (!rangeChunks.has(cell)) {
      const values = []; let current = cell.head;
      do { values.push(current); current += cell.step; }
      while (values.length < 32 && (cell.step > 0 ? current < cell.end : current > cell.end));
      const tail = (cell.step > 0 ? current < cell.end : current > cell.end) ? rangeNode(current, cell.end, cell.step, false) : emptyList;
      rangeChunks.set(cell, {chunk: arrayChunk(values), tail});
    }
    return rangeChunks.get(cell);
  }
  function ownedChunkRest(value) {
    const cell = cells.get(value);
    if (cell?.kind === "chunked-cons") return cell.more ?? emptyList;
    if (cell?.kind === "range") return cell.integer ? (cell.count <= 32 ? emptyList : rangeNode(cell.head + 32 * cell.step, cell.end, cell.step, true)) : forceRangeChunk(cell).tail;
    const slice = views.get(value);
    const end = Math.min(slice.storage.length, slice.offset + 32 - slice.offset % 32);
    return end < slice.storage.length ? view(slice.storage, end, slice.indexed, slice.kind, slice.tail) : slice.tail ?? emptyList;
  }
  function ChunkedCons(chunk, more, metadata, cachedHash) {
    const value = builtinValue("ChunkedCons", sequencePrototype);
    const chunkedState = {hash: cachedHash};
    cells.set(value, Object.freeze({kind: "chunked-cons", chunk, more, chunkedState,
      get hash() { return chunkedState.hash; },
      get head() { return nth(chunk, 0); },
      get tail() { return count(chunk) > 1 ? new ChunkedCons(core.dropFirstProtocol(chunk), more, null, null) : more ?? emptyList; }}));
    metadataValues.set(value, metadata);
    return value;
  }
  function chunkCons(chunk, tail) { return count(chunk) === 0 ? tail : new ChunkedCons(chunk, tail, null, null); }
  function dropOwned(value, n) {
    if (value == null) return null;
    if (maps.has(value)) return dropOwned(seq(value), n);
    const cell = cells.get(value);
    if (cell?.kind === "repeat") return cell.remaining === -1 ? value : repeat(cell.remaining - n, cell.head);
    if (cell?.kind === "range") return n > 0 ? n < cell.count ? rangeNode(cell.head + cell.step * n, cell.end, cell.step, true) : null : value;
    const items = vectors.get(value), slice = views.get(value);
    const storage = items ?? slice.storage, offset = (items ? 0 : slice.offset) + n;
    return offset < storage.length ? view(storage, offset, items ? items.length <= 32 : slice.indexed, items ? items.length <= 32 ? "indexed" : "chunked" : slice.kind) : null;
  }
  function numberArgument(value) {
    if (typeof value !== "number") throw new Error("Assert failed: (number? n)");
  }
  function collectionArity(name, args) {
    if (args !== 2) return invalidArity(args);
  }
  function take(n, coll) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    numberArgument(n);
    if (arguments.length === 1) return transform(rf => {
      let remaining = n;
      return reducingFunction(rf, (result, input) => {
        const prior = remaining; remaining--;
        const ret = prior > 0 ? rf.call(null, result, input) : result;
        return remaining > 0 ? ret : ensureReduced(ret);
      });
    });
    collectionArity("take", arguments.length);
    return lazySequence(() => {
      const s = n > 0 ? seq(coll) : null;
      return s === null ? null : cons(first(s), take(n - 1, rest(s)));
    });
  }
  function drop(n, coll) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    numberArgument(n);
    if (arguments.length === 1) return transform(rf => {
      let remaining = n;
      return reducingFunction(rf, (result, input) => { const prior = remaining; remaining--; return prior > 0 ? result : rf.call(null, result, input); });
    });
    collectionArity("drop", arguments.length);
    if (implementsProtocol(IDrop, coll)) return (n > 0 ? core.dropProtocol(coll, Math.ceil(n)) : seq(coll)) ?? emptyList;
    return lazySequence(() => { let s = seq(coll), remaining = n; while (remaining > 0 && s !== null) { s = seq(rest(s)); remaining--; } return s; });
  }
  function mapSeq(fn, ...colls) {
    if (arguments.length === 0) return lazySequence(() => null);
    if (arguments.length === 1) return transform(rf => callable(2, {
      0: () => rf.call(null), 1: result => rf.call(null, result),
      2: (result, input) => rf.call(null, result, fn.call(null, input))
    }, (result, input, inputs) => rf.call(null, result, apply(fn, input, inputs))));
    if (!colls.length) return invalidArity(arguments.length);
    // The variadic branch maps a lazy sequence of argument tuples and uses
    // apply. This preserves bounded rest arguments and host receiver behavior.
    if (colls.length > 3) return mapSeq(args => apply(fn, args), mapManySteps(listFromArray(colls)));
    return lazySequence(() => {
      const sources = [];
      for (const coll of colls) { const s = seq(coll); sources.push(s); if (colls.length > 3 && s === null) return null; }
      if (sources.some(value => value === null)) return null;
      if (sources.length === 1 && implementsProtocol(IChunkedSeq, sources[0])) {
        const chunk = chunkFirst(sources[0]), size = count(chunk), items = [];
        for (let i = 0; i < size; i++) items.push(fn.call(null, nth(chunk, i)));
        return chunkCons(arrayChunk(items), mapSeq(fn, chunkRest(sources[0])));
      }
      return cons(fn.call(null, ...sources.map(first)), mapSeq(fn, ...sources.map(rest)));
    });
  }
  function mapManySteps(collections) {
    return lazySequence(() => {
      const sources = mapSeq(seq, collections);
      return every(x => x, sources) ? cons(mapSeq(first, sources), mapManySteps(mapSeq(rest, sources))) : null;
    });
  }
  function indexedSequence(fn, coll, index, keepOnly, indexed = true) {
    return lazySequence(() => {
      const s = seq(coll), at = index;
      if (s !== null) {
        if (implementsProtocol(IChunkedSeq, s)) {
          const chunk = chunkFirst(s), size = count(chunk), values = [];
          for (let i = 0; i < size; i++) { const value = indexed ? fn.call(null, at + i, nth(chunk, i)) : fn.call(null, nth(chunk, i)); if (!keepOnly || value != null) values.push(value); }
          return chunkCons(arrayChunk(values), indexedSequence(fn, chunkRest(s), at + size, keepOnly, indexed));
        }
        const value = indexed ? fn.call(null, at, first(s)) : fn.call(null, first(s)), tail = rest(s);
        if (!keepOnly || value != null) return cons(value, indexedSequence(fn, tail, at + 1, keepOnly, indexed));
        return indexedSequence(fn, tail, at + 1, keepOnly, indexed);
      }
      return null;
    });
  }
  function mapIndexed(fn, coll) { if (arguments.length === 1) return indexedTransform(fn, false, true); collectionArity("map-indexed", arguments.length); return indexedSequence(fn, coll, 0, false); }
  function keepIndexed(fn, coll) { if (arguments.length === 1) return indexedTransform(fn, true, true); collectionArity("keep-indexed", arguments.length); return indexedSequence(fn, coll, 0, true); }
  function keep(fn, coll) { if (arguments.length === 1) return indexedTransform(fn, true, false); collectionArity("keep", arguments.length); return indexedSequence(fn, coll, 0, true, false); }
  function filter(pred, coll) {
    if (arguments.length === 1) return transform(rf => reducingFunction(rf, (result, input) => truth(pred.call(null, input)) ? rf.call(null, result, input) : result));
    collectionArity("filter", arguments.length);
    return lazySequence(() => {
      const s = seq(coll);
      if (s !== null) {
        if (implementsProtocol(IChunkedSeq, s)) {
          const chunk = chunkFirst(s), size = count(chunk), values = [];
          for (let i = 0; i < size; i++) { const value = nth(chunk, i); if (truth(pred.call(null, value))) values.push(value); }
          return chunkCons(arrayChunk(values), filter(pred, chunkRest(s)));
        }
        // Upstream obtains rest before invoking the predicate.
        const value = first(s), tail = rest(s);
        if (truth(pred.call(null, value))) return cons(value, filter(pred, tail));
        return filter(pred, tail);
      }
      return null;
    });
  }
  function remove(pred, coll) { if (arguments.length === 1) return filter(complement(pred)); collectionArity("remove", arguments.length); return filter(complement(pred), coll); }
  function takeWhile(pred, coll) {
    if (arguments.length === 1) return transform(rf => reducingFunction(rf, (result, input) => truth(pred.call(null, input)) ? rf.call(null, result, input) : reduced(result)));
    collectionArity("take-while", arguments.length);
    return lazySequence(() => { const s = seq(coll); return s !== null && truth(pred.call(null, first(s))) ? cons(first(s), takeWhile(pred, rest(s))) : null; });
  }
  function dropWhile(pred, coll) {
    if (arguments.length === 1) return transform(rf => {
      let dropping = true;
      return reducingFunction(rf, (result, input) => {
        if (dropping && truth(pred.call(null, input))) return result;
        dropping = false; return rf.call(null, result, input);
      });
    });
    collectionArity("drop-while", arguments.length);
    return lazySequence(() => { let s = seq(coll); while (s !== null && truth(pred.call(null, first(s)))) s = seq(rest(s)); return s; });
  }
  function repeatedly(n, fn) {
    if (arguments.length === 1) { fn = n; return lazySequence(() => cons(fn.call(null), repeatedly(fn))); }
    if (arguments.length === 2) return take(n, repeatedly(fn));
    return invalidArity(arguments.length);
  }
  function replicate(n, value) { return take(n, repeat(value)); }
  function takeNth(n, coll) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    numberArgument(n);
    if (arguments.length === 1) return transform(rf => {
      let index = -1;
      return reducingFunction(rf, (result, input) => ++index % n === 0 ? rf.call(null, result, input) : result);
    });
    collectionArity("take-nth", arguments.length);
    return lazySequence(() => { const s = seq(coll); return s === null ? null : cons(first(s), takeNth(n, drop(n, s))); });
  }
  function dropLast(n, coll) { if (arguments.length === 1) { coll = n; n = 1; } else if (arguments.length !== 2) return invalidArity(arguments.length); return mapSeq((x, _) => x, coll, drop(n, coll)); }
  function takeLast(n, coll) { let s = seq(coll), lead = seq(drop(n, coll)); while (lead !== null) { s = next(s); lead = next(lead); } return s; }
  function splitAt(n, coll) { return vector(take(n, coll), drop(n, coll)); }
  function splitWith(pred, coll) { return vector(takeWhile(pred, coll), dropWhile(pred, coll)); }
  function splitVAt(n, coll) { return vector(vec(take(n, coll)), drop(n, coll)); }
  function partitionV(n, step, pad, coll) {
    const arity = arguments.length;
    if (arity === 2) { coll = step; step = n; } else if (arity === 3) coll = pad; else if (arity !== 4) return invalidArity(arity);
    return lazySequence(() => {
      const s = seq(coll); if (s === null) return null;
      const p = vec(take(n, s));
      if (n === count(p)) return cons(p, arity === 4 ? partitionV(n, step, pad, nthrest(s, step)) : partitionV(n, step, nthrest(s, step)));
      return arity === 4 ? list(vec(take(n, concat(p, pad)))) : null;
    });
  }
  function partitionVAll(n, step, coll) {
    if (arguments.length === 2) { coll = step; step = n; } else if (arguments.length === 1) return partitionAll(n); else if (arguments.length !== 3) return invalidArity(arguments.length);
    return lazySequence(() => { const s = seq(coll); return s === null ? null : cons(vec(take(n, coll)), partitionVAll(n, step, drop(step, s))); });
  }
  function shuffle(coll) {
    const items = toArray(coll);
    for (let i = items.length - 1; i > 0; i--) { const j = Math.floor(Math.random() * (i + 1)); [items[i], items[j]] = [items[j], items[i]]; }
    return vectorFromArray(items);
  }
  const jsIterable = value => value != null && value[Symbol.iterator] != null;
  function iteration(step, ...options) {
    const opts = seqToMap(listFromArray(options)), option = (name, fallback) => get(opts, keyword(null, name), fallback);
    const somef = option("somef", value => value != null), vf = option("vf", value => value), kf = option("kf", value => value), initk = option("initk", null);
    const sequence = ret => {
      if (!truth(somef.call(null, ret))) return null;
      const value = vf.call(null, ret), key = kf.call(null, ret);
      return cons(value, key == null ? null : lazySequence(() => sequence(step.call(null, key))));
    };
    return installMethods({}, [["ISeqable", 0, null], ["IReduce", 0, null],
      ["ISeqable/-seq", 1, () => sequence(step.call(null, initk))],
      ["IReduce/-reduce", 3, (_self, rf, init) => {
        let acc = init, ret = step.call(null, initk);
        while (truth(somef.call(null, ret))) {
          acc = rf.call(null, acc, vf.call(null, ret));
          if (isReduced(acc)) return deref(acc);
          const key = kf.call(null, ret); if (key == null) return acc;
          ret = step.call(null, key);
        }
        return acc;
      }]]);
  }
  function partition(n, step, pad, coll) {
    const arity = arguments.length;
    if (arity === 2) { coll = step; step = n; } else if (arity === 3) coll = pad; else if (arity !== 4) return invalidArity(arity);
    return lazySequence(() => {
      const s = seq(coll); if (s === null) return null;
      const p = take(n, s);
      if (n === count(p)) return cons(p, arity === 4 ? partition(n, step, pad, drop(step, s)) : partition(n, step, drop(step, s)));
      return arity === 4 ? list(take(n, concat(p, pad))) : null;
    });
  }
  function partitionAll(n, step, coll) {
    if (arguments.length === 2) { coll = step; step = n; } else if (arguments.length === 1) return bufferedTransform({kind: "count", n}); else if (arguments.length !== 3) return invalidArity(arguments.length);
    return lazySequence(() => { const s = seq(coll); return s === null ? null : cons(take(n, s), partitionAll(n, step, drop(step, s))); });
  }
  function partitionBy(fn, coll) {
    if (arguments.length === 1) return bufferedTransform({kind: "key", fn});
    collectionArity("partition-by", arguments.length);
    return lazySequence(() => {
      const s = seq(coll); if (s === null) return null;
      const value = first(s), key = fn.call(null, value), run = cons(value, takeWhile(x => equalPair(key, fn.call(null, x)), next(s)));
      return cons(run, partitionBy(fn, lazySequence(() => drop(count(run), s))));
    });
  }
  function interleave(...colls) {
    if (!colls.length) return emptyList;
    if (colls.length === 1) return lazySequence(() => colls[0]);
    return lazySequence(() => {
      const ss = [];
      for (const coll of colls) { const s = seq(coll); ss.push(s); if (colls.length > 2 && s === null) return null; }
      if (ss.some(s => s === null)) return null;
      let result = interleave(...ss.map(rest));
      for (let i = ss.length - 1; i >= 0; i--) result = cons(first(ss[i]), result);
      return result;
    });
  }
  function interpose(sep, coll) {
    if (arguments.length === 1) return transform(rf => {
      let started = false;
      return reducingFunction(rf, (result, input) => {
        if (started) { result = rf.call(null, result, sep); if (isReduced(result)) return result; }
        else started = true;
        return rf.call(null, result, input);
      });
    });
    collectionArity("interpose", arguments.length);
    return drop(1, interleave(repeat(sep), coll));
  }
  function dorun(n, coll) {
    if (arguments.length === 1) { coll = n; n = Infinity; } else if (arguments.length !== 2) return invalidArity(arguments.length);
    while (seq(coll) !== null && n > 0) { coll = next(coll); n--; }
    return null;
  }
  function doall(n, coll) { if (arguments.length === 1) { dorun(n); return n; } if (arguments.length === 2) { dorun(n, coll); return coll; } return invalidArity(arguments.length); }
  function last(coll) { let s = seq(coll); while (s !== null) { const tail = next(s); if (tail === null) return first(s); s = tail; } return null; }
  function butlast(coll) { const values = toArray(coll); values.pop(); return seq(vectorFromArray(values)); }
  function mapv(fn, ...colls) { return into(vector(), mapSeq(fn, ...colls)); }
  function filterv(fn, coll) { return reduce((v, x) => truth(fn.call(null, x)) ? conj(v, x) : v, vector(), coll); }
  function cat(rf) {
    return reducingFunction(rf, (result, input) => reduce((acc, value) => {
      const ret = rf.call(null, acc, value);
      return isReduced(ret) ? reduced(ret) : ret;
    }, result, input));
  }
  function mapcat(fn, ...colls) {
    if (arguments.length === 1) return comp(mapSeq(fn), cat);
    if (!colls.length) return invalidArity(arguments.length);
    return apply(concat, mapSeq(fn, ...colls));
  }
  function rangeNode(start, end, step, integer, suppliedCount) {
    const rangeCount = arguments.length >= 5 ? suppliedCount : Math.ceil((end - start) / step);
    const value = builtinValue(integer ? "IntegerRange" : "Range", sequencePrototype);
    cells.set(value, Object.freeze({kind: "range", head: start, end, step, integer,
      ...(integer ? {count: rangeCount} : {}),
      get tail() { const following = start + step; return (step > 0 ? following < end : following > end) ? rangeNode(following, end, step, integer) : emptyList; }}));
    return value;
  }
  function range(start, end, step = 1) {
    if (arguments.length === 0) { start = 0; end = Number.MAX_VALUE; }
    if (arguments.length === 1) { end = start; start = 0; }
    if (arguments.length > 3) return invalidArity(arguments.length);
    if (step > 0 ? end <= start : step < 0 ? end >= start : end === start) return emptyList;
    if (!(step > 0 || step < 0)) return repeat(start);
    return rangeNode(start, end, step, isInteger(start) && isInteger(end) && isInteger(step));
  }
  // Construct the same fixed/variadic split as an emitted CLJS function. This
  // lets combinators hand an unbounded tail to a variadic target unchanged.
  function callable(fixed, methods, variadic) {
    const fn = function (...args) {
      const method = methods[args.length];
      return method ? method(...args) : variadic(...Array.from({length: fixed}, (_, i) => args[i]), fnRest(args, fixed));
    };
    for (const [arity, method] of Object.entries(methods)) fn["cljs$core$IFn$_invoke$arity$" + arity] = method;
    fn.cljs$lang$maxFixedArity = fixed;
    fn.cljs$lang$applyTo = args => fnApplyTo(variadic, fixed, args);
    fn.cljs$core$IFn$_invoke$arity$variadic = variadic;
    return fn;
  }
  function directOrApply(fn, args, tail) { return tail === missing ? fn.call(null, ...args) : apply(fn, ...args, tail); }
  function complement(fn) {
    const negate = value => value == null || value === false;
    return callable(2, {0: () => negate(fn.call(null)), 1: x => negate(fn.call(null, x)), 2: (x, y) => negate(fn.call(null, x, y))},
      (x, y, zs) => negate(apply(fn, x, y, zs)));
  }
  function constantly(x) { return callable(2, {0: () => x, 1: () => x, 2: () => x}, () => x); }
  function predicateCombination(predicates, small, existential) {
    // The first three arguments visit each predicate in turn. CLJS's 1-3
    // predicate variants then visit each tail argument; the 4+ variant instead
    // traverses that tail once per predicate. Neither tail is materialized.
    const fixed = args => {
      if (!args.length) return existential ? null : true;
      let result = null;
      for (let ps = seq(predicates); ps !== null; ps = next(ps)) {
        const predicate = first(ps);
        for (const arg of args) {
          result = predicate.call(null, arg);
          if (truth(result) === existential) return existential ? result : false;
        }
      }
      // Fixed-arity some-fn with 1-3 predicates returns the final false/nil
      // value; its variadic and 4+ predicate variants use some, returning nil.
      return existential ? small ? result : null : true;
    };
    const tail = args => {
      if (small) {
        for (let xs = seq(args); xs !== null; xs = next(xs)) {
          const value = first(xs);
          for (let ps = seq(predicates); ps !== null; ps = next(ps)) {
            const result = first(ps).call(null, value);
            if (truth(result) === existential) return existential ? result : false;
          }
        }
      } else {
        for (let ps = seq(predicates); ps !== null; ps = next(ps)) {
          const predicate = first(ps);
          for (let xs = seq(args); xs !== null; xs = next(xs)) {
            const result = predicate.call(null, first(xs));
            if (truth(result) === existential) return existential ? result : false;
          }
        }
      }
      return existential ? null : true;
    };
    return callable(3, {0: () => existential ? null : true, 1: x => fixed([x]),
      2: (x, y) => fixed([x, y]), 3: (x, y, z) => fixed([x, y, z])},
    (x, y, z, args) => {
      const result = fixed([x, y, z]);
      return truth(result) === existential ? result : tail(args);
    });
  }
  function predicateConstructor(existential) {
    const build = (...ps) => predicateCombination(listFromArray(ps), true, existential);
    return callable(3, {1: p => build(p), 2: (p, q) => build(p, q), 3: (p, q, r) => build(p, q, r)},
      (p, q, r, ps) => predicateCombination(cons(p, cons(q, cons(r, ps))), false, existential));
  }
  const someFn = predicateConstructor(true), everyPred = predicateConstructor(false);
  function memoize(fn) {
    let cache = mapFromArray([]);
    return callable(0, {}, args => {
      const cached = get(cache, args, missing);
      if (cached !== missing) return cached;
      const result = apply(fn, args);
      cache = assoc(cache, args, result);
      return result;
    });
  }
  function trampolineLoop(fn) {
    for (;;) {
      const result = fn.call(null);
      if (!predicates.isFn(result)) return result;
      fn = result;
    }
  }
  const trampoline = callable(1, {1: trampolineLoop}, (fn, args) => trampolineLoop(() => apply(fn, args)));
  function comp(...fns) {
    if (!fns.length) return predicates.identity;
    if (fns.length === 1) return fns[0];
    const run = (args, tail) => {
      let result = directOrApply(fns[fns.length - 1], args, tail);
      for (let i = fns.length - 2; i >= 0; i--) result = fns[i].call(null, result);
      return result;
    };
    if (fns.length > 3) return callable(0, {}, args => run([], args));
    return callable(3, {0: () => run([], missing), 1: x => run([x], missing), 2: (x, y) => run([x, y], missing), 3: (x, y, z) => run([x, y, z], missing)},
      (x, y, z, tail) => run([x, y, z], tail));
  }
  function partialFixed(fn, prefix) {
    if (!prefix.length) return fn;
    const run = (...args) => fn.call(null, ...prefix, ...args);
    return callable(3, {0: () => run(), 1: x => run(x), 2: (x, y) => run(x, y), 3: (x, y, z) => run(x, y, z)},
      (x, y, z, tail) => apply(fn, ...prefix, x, y, z, tail));
  }
  // Constructors need the same sequence ABI as their returned functions: apply
  // may supply an infinite tail which construction must only capture.
  const partial = callable(4, {
    0: () => partialFixed(undefined, [undefined, undefined, undefined]),
    1: fn => fn,
    2: (fn, a) => partialFixed(fn, [a]),
    3: (fn, a, b) => partialFixed(fn, [a, b]),
    4: (fn, a, b, c) => partialFixed(fn, [a, b, c])
  }, (fn, a, b, c, more) => callable(0, {}, args => apply(fn, a, b, c, concat(more, args))));
  function juxtFixed(fns) {
    const run = (args, tail) => vectorFromArray(fns.map(fn => directOrApply(fn, args, tail)));
    return callable(3, {0: () => run([], missing), 1: x => run([x], missing), 2: (x, y) => run([x, y], missing), 3: (x, y, z) => run([x, y, z], missing)},
      (x, y, z, tail) => run([x, y, z], tail));
  }
  const juxt = callable(3, {
    0: () => juxtFixed([undefined, undefined, undefined]),
    1: f => juxtFixed([f]),
    2: (f, g) => juxtFixed([f, g]),
    3: (f, g, h) => juxtFixed([f, g, h])
  }, (f, g, h, more) => {
    const fns = listStar(f, g, h, more);
    const run = (args, tail) => vectorFromArray(toArray(mapSeq(fn => directOrApply(fn, args, tail), fns)));
    return callable(3, {0: () => run([], missing), 1: x => run([x], missing), 2: (x, y) => run([x, y], missing), 3: (x, y, z) => run([x, y, z], missing)},
      (x, y, z, tail) => run([x, y, z], tail));
  });
  function fnil(fn, ...defaults) {
    if (defaults.length < 1 || defaults.length > 3) return invalidArity(arguments.length);
    const patch = args => args.map((x, i) => i < defaults.length && x == null ? defaults[i] : x);
    const run = (...args) => fn.call(null, ...patch(args));
    const methods = {2: (x, y) => run(x, y), 3: (x, y, z) => run(x, y, z)};
    if (defaults.length === 1) methods[1] = x => run(x);
    return callable(3, methods, (x, y, z, tail) => apply(fn, ...patch([x, y, z]), tail));
  }
  function into(to, from, source) {
    switch (arguments.length) {
      case 0: return vector();
      case 1: return to;
      case 2: {
        if (implementsProtocol(IEditableCollection, to))
          return core.withMetaProtocol(persistentBang(reduce(core.conjBangProtocol, transient(to), from)), meta(to));
        return reduce(to == null ? conj : core.conjProtocol, to, from);
      }
      case 3: {
        const editable = implementsProtocol(IEditableCollection, to), metadata = editable ? meta(to) : null;
        const rf = !editable ? conj : function (...args) {
          if (args.length === 1) return core.withMetaProtocol(persistentBang(args[0]), metadata);
          if (args.length === 2) return conjBang(args[0], args[1]);
          return invalidArity(args.length);
        };
        return transduce(from, rf, editable ? transient(to) : to, source);
      }
      default: return invalidArity(arguments.length);
    }
  }
  // Match the CLJS constructor: preserve the supplied message/data/cause values
  // and Error identity, including when callers construct ExceptionInfo directly.
  const exceptionMessages = new WeakMap();
  const compilerExceptionMessage = value => exceptionMessages.get(value);
  function ExceptionInfo(message, data, cause) {
    const error = new Error(message);
    const target = this == null ? globalThis : Object(this);
    target.message = message;
    target.data = data;
    target.cause = cause;
    if (typeof message === "string") exceptionMessages.set(target, message);
    for (const key of ["name", "description", "number", "fileName", "lineNumber", "columnNumber", "stack"])
      target[key] = error[key];
    return target;
  }
  Object.setPrototypeOf(ExceptionInfo.prototype, Error.prototype);
  ExceptionInfo.prototype.toString = function () {
    return "#error {:message " + print(this.message, true)
      + (truth(this.data) ? ", :data " + print(this.data, true) : "")
      + (truth(this.cause) ? ", :cause " + print(this.cause, true) : "") + "}";
  };
  function exInfo(message, data, cause) {
    if (arguments.length !== 2 && arguments.length !== 3) return invalidArity(arguments.length);
    return new ExceptionInfo(message, data, arguments.length === 2 ? null : cause);
  }
  const exData = value => value instanceof ExceptionInfo ? value.data : null;
  const exMessage = value => value instanceof Error ? value.message : null;
  const exCause = value => value instanceof ExceptionInfo ? value.cause : null;
  function throwableToMap(error) {
    const k = name => keyword(null, name), via = [];
    let root = null;
    for (let current = error; truth(current); current = exCause(current)) {
      root = current;
      const pairs = [k("type"), current instanceof ExceptionInfo ? symbol("cljs.core", "ExceptionInfo")
        : current instanceof Error ? symbol("js", current.name) : null];
      const message = exMessage(current), data = exData(current);
      if (truth(message)) pairs.push(k("message"), message);
      if (truth(data)) pairs.push(k("data"), data);
      via.push(mapFromArray(pairs));
    }
    const pairs = [k("via"), vectorFromArray(via), k("trace"), null];
    const message = exMessage(root), data = exData(root);
    const phase = get(exData(error), keyword("clojure.error", "phase"));
    if (truth(message)) pairs.push(k("cause"), message);
    if (truth(data)) pairs.push(k("data"), data);
    if (truth(phase)) pairs.push(k("phase"), phase);
    return mapFromArray(pairs);
  }
  function printCore(...values) { return printItems(values); }
  function setPrintFn(fn) { if (arguments.length !== 1) return invalidArity(arguments.length); return core.printFn = fn; }
  function setPrintErrFn(fn) { if (arguments.length !== 1) return invalidArity(arguments.length); return core.printErrFn = fn; }
  function enableConsolePrint() {
    if (arguments.length !== 0) return invalidArity(arguments.length);
    core.printNewline = false;
    setPrintFn((...args) => globalThis.console.log(...args));
    setPrintErrFn((...args) => globalThis.console.error(...args));
    return null;
  }
  function stringPrint(text) {
    if (arguments.length !== 1) return invalidArity(arguments.length);
    if (core.printFn == null) throw new Error("No *print-fn* fn set for evaluation environment");
    core.printFn.call(null, text); return null;
  }
  function printOption(options, name, root) { return options == null ? core[root] : get(options, keyword(null, name)); }
  function flush() { if (arguments.length !== 0) return invalidArity(arguments.length); return null; }
  function newline(options = null) {
    if (arguments.length > 1) return invalidArity(arguments.length);
    stringPrint("\n");
    return truth(printOption(options, "flush-on-newline", "flushOnNewline")) ? core.flush.call(null) : null;
  }
  function pr(...values) { return stringPrint(prStr(...values)); }
  function println(...values) { printItems(values); return truth(core.printNewline) ? newline() : null; }
  function prn(...values) { pr(...values); return truth(core.printNewline) ? newline() : null; }
  function prnStr(...values) { return prStr(...values) + "\n"; }
  function printStr(...values) { const saved = core.printReadably; core.printReadably = false; try { return prStr(...values); } finally { core.printReadably = saved; } }
  function printlnStr(...values) { return printStr(...values) + "\n"; }
  function TaggedLiteral(tag, form) {
    this.tag = tag; this.form = form;
    installMethods(this, [["IEquiv", 0, null], ["IHash", 0, null], ["ILookup", 0, null], ["IPrintWithWriter", 0, null],
      ["IEquiv/-equiv", 2, (_self, other) => other instanceof TaggedLiteral && equalPair(tag, other.tag) && equalPair(form, other.form)],
      ["IHash/-hash", 1, () => 31 * hash(tag) + hash(form)],
      ["ILookup/-lookup", 2, (_self, key) => equalPair(key, keyword(null, "tag")) ? tag : equalPair(key, keyword(null, "form")) ? form : null],
      ["ILookup/-lookup", 3, (_self, key, notFound) => equalPair(key, keyword(null, "tag")) ? tag : equalPair(key, keyword(null, "form")) ? form : notFound],
      ["IPrintWithWriter/-pr-writer", 3, (_self, writer, options) => { write(writer, "#" + str(tag) + " "); return printWriter(form, writer, options); }]]);
  }
  TaggedLiteral.prototype.toString = function () { return prStr(this); };
  function taggedLiteral(tag, form) { if (!symbols.has(tag)) throw new Error("Assert failed: (symbol? tag)"); return new TaggedLiteral(tag, form); }
  function printItems(values) {
    const saved = core.printReadably;
    core.printReadably = false;
    try { return stringPrint(prStr(...values)); }
    finally { core.printReadably = saved; }
  }
  function stringWriter() {
    const buffer = {text: ""};
    const writer = installMethods({}, [["IWriter", 0, null],
      ["IWriter/-write", 2, (_self, text) => { buffer.text += text; return null; }], ["IWriter/-flush", 1, () => null]]);
    return {buffer, writer};
  }
  function printMetaQ(options, value) {
    if (arguments.length !== 2) return invalidArity(arguments.length);
    return truth(printOption(options, "meta", "printMeta")) && implementsProtocol(IMeta, value) && meta(value) != null;
  }
  function prSequentialWriter(writer, printOne, begin, separator, end, options, coll) {
    if (arguments.length !== 7) return invalidArity(arguments.length);
    const saved = core.printLevel;
    core.printLevel = saved == null ? null : saved - 1;
    try {
      if (core.printLevel != null && core.printLevel < 0) return write(writer, "#");
      write(writer, begin);
      let current = seq(coll), remaining = printOption(options, "print-length", "printLength"), printed = false;
      while (current != null && (remaining == null || remaining !== 0)) {
        if (printed) write(writer, separator);
        printOne.call(null, first(current), writer, options); printed = true;
        current = next(current); if (remaining != null) remaining--;
      }
      if (current != null) {
        if (printed) write(writer, separator);
        const marker = get(options, keyword(null, "more-marker")); write(writer, truth(marker) ? marker : "...");
      }
      return write(writer, end);
    } finally { core.printLevel = saved; }
  }
  function printMapEntries(entries, writer, options, prefix = "") {
    const names = entries.map(entry => keywords.get(entry.key) || symbols.get(entry.key));
    const ns = truth(core.printNamespaceMaps) && names.length && names[0]?.namespace;
    const lifted = !prefix && ns && names.every(name => name?.namespace === ns);
    return prSequentialWriter(writer, (entry, target, opts) => {
      const key = lifted ? keywords.has(entry.key) ? keyword(null, name(entry.key)) : symbol(null, name(entry.key)) : entry.key;
      printWriter(key, target, opts); write(target, " "); return printWriter(entry.value, target, opts);
    }, prefix + (lifted ? "#:" + ns : "") + "{", ", ", "}", options, entries);
  }
  function printWriter(value, writer, options) {
    const alt = get(options, keyword(null, "alt-impl"));
    if (truth(alt)) return alt.call(null, value, writer, assoc(options, keyword(null, "fallback-impl"), printWriterImpl));
    return printWriterImpl(value, writer, options);
  }
  function printWriterImpl(value, writer, options, skipMetadata = false) {
    if (value == null) return write(writer, "nil");
    if (!skipMetadata && printMetaQ(options, value)) { write(writer, "^"); printWriter(meta(value), writer, options); write(writer, " "); }
    const writerMethod = publicMethod(value, "IPrintWithWriter/-pr-writer", 3);
    if (writerMethod) return writerMethod.call(value, value, writer, options);
    const readable = truth(printOption(options, "readably", "printReadably"));
    if (typeof value === "string") {
      // CLJS escapes its seven named characters and preserves other UTF-16 units.
      const escapes = {'"': '\\"', '\\': '\\\\', '\b': '\\b', '\f': '\\f', '\n': '\\n', '\r': '\\r', '\t': '\\t'};
      return write(writer, readable ? '"' + value.replace(/[\\"\b\f\n\r\t]/g, ch => escapes[ch]) + '"' : value);
    }
    if (typeof value === "number") return write(writer, Number.isNaN(value) ? "##NaN" : value === Infinity ? "##Inf" : value === -Infinity ? "##-Inf" : Object.is(value, -0) ? "-0.0" : String(value));
    if (typeof value === "boolean") return write(writer, String(value));
    if (typeof value === "function") return write(writer, value.cljs$lang$type && typeNames.has(value) ? typeNames.get(value) : "#object[" + (value.name || "Function") + (truth(core.printFnBodies) ? ' "' + String(value) + '"' : "") + "]");
    if (value instanceof ExceptionInfo) return write(writer, value.toString());
    if (transients.has(value)) {
      const state = transients.get(value);
      return write(writer, "#object[cljs.core." + (state.kind === "vector" ? "TransientVector" : state.kind === "set" ? "TransientHashSet" : state.mapKind === "array-map" ? "TransientArrayMap" : "TransientHashMap") + "]");
    }
    if (atoms.has(value) || volatiles.has(value)) {
      write(writer, "#object[cljs.core." + (atoms.has(value) ? "Atom" : "Volatile") + " {:val "); printWriter(value.state, writer, options); return write(writer, "}]");
    }
    if (delays.has(value)) {
      const state = delays.get(value);
      write(writer, "#object[cljs.core.Delay {:status " + (state.thunk == null ? ":ready" : ":pending") + ", :val "); printWriter(state.value, writer, options); return write(writer, "}]");
    }
    if (value instanceof Error) return write(writer, "#object[" + (value.constructor?.name || "Object") + " " + String(value) + "]");
    if (keywords.has(value) || symbols.has(value) || vars.has(value)) return write(writer, String(value));
    if (uuids.has(value)) return write(writer, '#uuid "' + uuids.get(value) + '"');
    if (value instanceof Date) return write(writer, '#inst "' + value.toISOString().replace(/Z$/, "-00:00") + '"');
    if (value instanceof RegExp) return write(writer, '#"' + value.source + '"');
    if (sequential(value) || queues.has(value)) {
      const [start, end] = queues.has(value) ? ["#queue [", "]"] : vectors.has(value) ? ["[", "]"] : ["(", ")"];
      return prSequentialWriter(writer, printWriter, start, " ", end, options, value);
    }
    const entries = maps.get(value);
    if (entries) return printMapEntries(entries, writer, options);
    const members = sets.get(value);
    if (members) return prSequentialWriter(writer, (e, target, opts) => printWriter(e.key, target, opts), "#{", " ", "}", options, members);
    if (Array.isArray(value)) return prSequentialWriter(writer, printWriter, "#js [", " ", "]", options, value);
    if (typeof value === "object" && Object.getPrototypeOf(value) === null) return write(writer, "#object[Object]");
    if (typeof value === "object" && value.constructor === Object) return printMapEntries(Object.keys(value).map(k => ({key: /^[A-Za-z_*+?!\-'][\w*+?!\-']*$/.test(k) ? keyword(null, k) : k, value: value[k]})), writer, options, "#js ");
    if (typeof value === "symbol") return write(writer, "#object[" + String(value) + "]");
    return write(writer, "#object[" + (value.constructor?.cljs$lang$ctorStr?.replaceAll("/", ".") || value.constructor?.name || "Object") + " " + String(value) + "]");
  }
  function prSeqWriter(values, writer, options) {
    if (arguments.length !== 3) return invalidArity(arguments.length);
    printWriter(first(values), writer, options);
    for (let s = next(values); s != null; s = next(s)) { write(writer, " "); printWriter(first(s), writer, options); }
    return null;
  }
  function prStrWithOpts(values, options) {
    if (arguments.length !== 2) return invalidArity(arguments.length);
    if (seq(values) == null) return "";
    const {buffer, writer} = stringWriter(); prSeqWriter(values, writer, options); flushWriter(writer); return buffer.text;
  }
  function prnStrWithOpts(values, options) { if (arguments.length !== 2) return invalidArity(arguments.length); return prStrWithOpts(values, options) + "\n"; }
  function print(value, readable) {
    const saved = core.printReadably; core.printReadably = readable;
    try { const {buffer, writer} = stringWriter(); printWriterImpl(value, writer, null, true); flushWriter(writer); return buffer.text; }
    finally { core.printReadably = saved; }
  }
  function prStr(...values) { return prStrWithOpts(values, null); }
  // The Rust display API returns UTF-8. Keep canonical JS printing lossless,
  // but represent isolated surrogate units as escapes at this text boundary.
  function printForHost(value) {
    return prStr(value).replace(/[\uD800-\uDBFF][\uDC00-\uDFFF]|[\uD800-\uDFFF]/g,
      units => units.length === 2 ? units : "\\u" + units.charCodeAt(0).toString(16).padStart(4, "0"));
  }
  function str(...values) { return values.map(value => value == null ? "" : typeof value === "string" ? value : sequential(value) || (maps.has(value) && !recordTypes.has(value)) || sets.has(value) ? print(value, true) : String(value)).join(""); }

  function variadicCore(fn, fixed, delegate) {
    fn.cljs$lang$maxFixedArity = fixed;
    fn.cljs$lang$applyTo = args => fnApplyTo(delegate, fixed, args);
    fn.cljs$core$IFn$_invoke$arity$variadic = delegate;
    return fn;
  }
  function foldSeq(operation, x, y, more) {
    let result = operation(x, y);
    for (let s = seq(more); s !== null; s = next(s)) result = operation(result, first(s));
    return result;
  }
  variadicCore(add, 2, (x, y, more) => foldSeq((a, b) => a + b, x, y, more));
  variadicCore(subtract, 2, (x, y, more) => foldSeq((a, b) => a - b, x, y, more));
  variadicCore(multiply, 2, (x, y, more) => foldSeq((a, b) => a * b, x, y, more));
  variadicCore(divide, 2, (x, y, more) => foldSeq((a, b) => a / b, x, y, more));
  variadicCore(equal, 2, (x, y, more) => {
    if (!equalPair(x, y)) return false;
    for (let s = seq(more); s !== null; s = next(s)) {
      const item = first(s);
      if (!equalPair(y, item)) return false;
      y = item;
    }
    return true;
  });
  variadicCore(vector, 0, args => vectorFromArray(toArray(args)));
  variadicCore(list, 0, args => listFromArray(toArray(args)));
  variadicCore(hashMap, 0, args => mapFromArray(toArray(args), "hash-map"));
  variadicCore(arrayMap, 0, args => mapFromArray(toArray(args), "array-map"));
  variadicCore(hashSet, 0, args => setFromArray(toArray(args)));
  variadicCore(str, 1, (x, more) => {
    let result = str(x);
    for (let s = seq(more); s !== null; s = next(s)) result += str(first(s));
    return result;
  });

  variadicCore(printCore, 0, args => printItems(toArray(args)));
  variadicCore(minKey, 3, (key, x, y, more) => keyExtreme(true, key, x, y, more));
  variadicCore(maxKey, 3, (key, x, y, more) => keyExtreme(false, key, x, y, more));
  variadicCore(isDistinct, 2, distinctMore);

  variadicCore(swap, 4, (value, fn, x, y, more) => swapState(value, fn, cons(x, cons(y, more)), false, true));
  variadicCore(swapVals, 4, (value, fn, x, y, more) => swapState(value, fn, cons(x, cons(y, more)), true, true));
  variadicCore(merge, 0, values => mergeMaps(values, missing));
  variadicCore(mergeWith, 1, (fn, values) => mergeMaps(values, fn));
  function spreadArgs(args) {
    if (args == null) return null;
    const following = next(args), item = first(args);
    return following == null ? seq(item) : cons(item, spreadArgs(following));
  }
  variadicCore(listStar, 4, (a, b, c, d, more) => cons(a, cons(b, cons(c, cons(d, spreadArgs(more))))));
  variadicCore(updateIn, 6, (value, path, fn, x, y, z, more) => updatePath(value, path, old => apply(fn, old, x, y, z, more)));
  variadicCore(update, 6, (value, key, fn, x, y, z, more) => assoc(value, key, apply(fn, get(value, key), x, y, z, more)));

  // Host access follows JavaScript property conversion and receiver semantics.
  function jsSet(object, key, value) {
    if (object == null) throw new TypeError("Cannot set properties of null or undefined");
    Reflect.set(Object(object), key, value, object);
    return value;
  }
  function jsDelete(object, key) {
    if (object == null) throw new TypeError("Cannot delete properties of null or undefined");
    return Reflect.deleteProperty(Object(object), key);
  }
  function aget(array, index, ...indices) {
    let value = array[index];
    for (const nextIndex of indices) value = value[nextIndex];
    return value;
  }
  function aset(array, index, value, ...more) {
    if (more.length === 0) return jsSet(array, index, value);
    return aset(array[index], value, ...more);
  }
  function makeArray(type, size, ...dimensions) {
    if (arguments.length === 0) return invalidArity(0);
    if (arguments.length === 1) return new Array(type);
    const result = new Array(size);
    if (dimensions.length) for (let i = 0; i < result.length; i++) result[i] = makeArray(null, ...dimensions);
    return result;
  }
  function aclone(array) {
    const result = new Array(array.length);
    for (let i = 0; i < result.length; i++) result[i] = array[i];
    return result;
  }
  function toArray2d(coll) {
    const result = new Array(count(coll));
    let i = 0;
    for (let s = seq(coll); s !== null; s = next(s)) result[i++] = toArray(first(s));
    return result;
  }
  function typedArray(size, init) {
    if (arguments.length === 1) return typeof size === "number" ? typedArray(size, null) : intoArray(size);
    if (arguments.length !== 2) return invalidArity(arguments.length);
    const result = makeArray(size);
    if (predicates.isSeq(init)) {
      // The upstream loop advances after each store, even after the last slot.
      let s = seq(init);
      for (let i = 0; s !== null && i < size; i++, s = next(s)) result[i] = first(s);
    } else {
      for (let i = 0; i < size; i++) result[i] = init;
    }
    return result;
  }
  function intArray(...args) { return typedArray(...args); }
  function longArray(...args) { return typedArray(...args); }
  function doubleArray(...args) { return typedArray(...args); }
  function objectArray(...args) { return typedArray(...args); }
  function intoArray(type, coll) {
    if (arguments.length !== 1 && arguments.length !== 2) return invalidArity(arguments.length);
    return toArray(arguments.length === 1 ? type : coll);
  }
  function jsObj(...pairs) {
    // goog.object/create accepts a single array of interleaved entries.
    if (pairs.length === 1 && Array.isArray(pairs[0])) pairs = pairs[0];
    if (pairs.length % 2) throw new Error("Uneven number of arguments");
    const result = {};
    for (let i = 0; i < pairs.length; i += 2) result[pairs[i]] = pairs[i + 1];
    return result;
  }
  function jsKeys(object) { const keys = []; for (const key in object) keys.push(key); return keys; }
  function jsInvoke(object, key, ...args) { return object[key].apply(object, args); }
  function keyToJs(key, primitive = cljToJs) {
    if (satisfies(IEncodeJS, key)) return core.encodeJsProtocol(key);
    return typeof key === "string" || typeof key === "number" || keywords.has(key) || symbols.has(key) ? primitive.call(null, key) : prStr(key);
  }
  function cljToJs(value, ...options) {
    const opts = seqToMap(listFromArray(options)), keywordFn = get(opts, keyword(null, "keyword-fn"), name);
    const convert = value => {
      if (value == null) return null;
      if (satisfies(IEncodeJS, value)) return core.encodeJsProtocol(value);
      if (keywords.has(value)) return keywordFn.call(null, value);
      if (symbols.has(value)) return str(value);
      if (predicates.isMap(value)) {
        const result = {};
        for (let s = seq(value); s !== null; s = next(s)) { const entry = first(s); result[keyToJs(nth(entry, 0), convert)] = convert(nth(entry, 1)); }
        return result;
      }
      if (predicates.isColl(value)) return toArray(mapSeq(convert, value));
      return value;
    };
    return convert(value);
  }
  function jsToClj(value, ...options) {
    if (arguments.length === 1) options = [keyword(null, "keywordize-keys"), false];
    const opts = seqToMap(listFromArray(options)), keywordize = truth(get(opts, keyword(null, "keywordize-keys")));
    const convert = value => {
      if (satisfies(IEncodeClojure, value)) return core.encodeClojureProtocol(value, arrayMap(...options));
      if (predicates.isSeq(value)) return doall(mapSeq(convert, value));
      if (predicates.isMapEntry(value)) return mapEntry(convert(nth(value, 0)), convert(nth(value, 1)));
      if (predicates.isColl(value)) return into(empty(value), mapSeq(convert, value));
      if (Array.isArray(value)) return vectorFromArray(value.map(convert));
      if (value != null && value.constructor === Object) {
        let result = mapFromArray([]);
        for (const key of Object.keys(value)) result = assoc(result, keywordize ? keywordCore(key) : key, convert(value[key]));
        return result;
      }
      return value;
    };
    return convert(value);
  }
  function checkedIndex(array, index) {
    if (!(array != null && (Array.isArray(array) || (typeof array === "object" && typeof array.length === "number"))) ||
        typeof index !== "number" || index < 0 || !(index < array.length)) throw new Error("Assert failed: array index out of bounds");
  }
  function checkedAgetStrict(array, index, ...indices) {
    checkedIndex(array, index);
    const value = array[index];
    return indices.length ? checkedAgetStrict(value, ...indices) : value;
  }
  function checkedAsetStrict(array, index, value, ...more) {
    checkedIndex(array, index);
    if (more.length) return checkedAsetStrict(array[index], value, ...more);
    return jsSet(array, index, value);
  }

  function coreMethod(key, arities, fallback) {
    const dispatch = function (value, ...args) {
      const arity = arities.length === 1 ? arities[0] : arguments.length;
      if (!arities.includes(arity)) return invalidArity(arguments.length);
      if (arities.length === 1) args = args.slice(0, arity - 1);
      const method = builtin(value, key, arity);
      if (method != null) return method.call(value, value, ...args);
      const protocol = coreProtocolsByName.get(key.split("/")[0]);
      if (builtinCoreImplementation(protocol, value) || protocol === IHash || protocol === IEquiv || (protocol === ICounted && value == null)) return fallback(value, ...args);
      throw new Error("No protocol method " + key + " defined for this value");
    };
    coreDispatchers.set(key, dispatch);
    return dispatch;
  }
  const write = coreMethod("IWriter/-write", [2], () => null);
  const flushWriter = coreMethod("IWriter/-flush", [1], () => null);
  const prWriter = coreMethod("IPrintWithWriter/-pr-writer", [3], (value, writer, options) => printWriterImpl(value, writer, options, true));
  const clone = coreMethod("ICloneable/-clone", [1], value => {
    if (!cloneablePayload(value)) throw new Error("No protocol method ICloneable.-clone defined for this value");
    const result = Object.create(Object.getPrototypeOf(value));
    for (const store of [...protocolStores, metadataValues, mapKinds, setKinds, sortedComparators, subvecSources]) if (store.has(value)) store.set(result, store.get(value));
    if (mapEntries.has(value)) mapEntries.add(result);
    return result;
  });
  const seqProtocol = coreMethod("ISeqable/-seq", [1], seq), firstProtocol = coreMethod("ISeq/-first", [1], first), restProtocol = coreMethod("ISeq/-rest", [1], rest), nextProtocol = coreMethod("INext/-next", [1], next), countProtocol = coreMethod("ICounted/-count", [1], count), nthProtocol = coreMethod("IIndexed/-nth", [2, 3], function (value, index, notFound) { return transients.has(value) ? transientNth(value, index, arguments.length === 2 ? missing : notFound) : nth(...arguments); }), lookupProtocol = coreMethod("ILookup/-lookup", [2, 3], get), containsProtocol = coreMethod("IAssociative/-contains-key?", [2], contains);
  for (const [names, base] of [
    [["PersistentVector", "Subvec", "MapEntry"], vectorPrototype],
    [["PersistentArrayMap", "PersistentHashMap", "PersistentTreeMap"], mapPrototype],
    [["PersistentHashSet", "PersistentTreeSet"], setPrototype], [["List", "EmptyList", "Cons", "IndexedSeq", "ChunkedSeq", "ChunkedCons", "LazySeq", "PersistentQueue", "Repeat", "Cycle", "Iterate", "Range", "IntegerRange", "NodeSeq", "PersistentTreeMapSeq"], sequencePrototype],
    [["Keyword"], keywordPrototype], [["Symbol"], symbolPrototype], [["Reduced"], reducedPrototype], [["UUID"], uuidPrototype]
  ]) for (const name of names) builtinType(name, base);
  Object.defineProperty(builtinTypes.PersistentTreeMap.prototype, "comp", {get() { return sortedComparators.get(this); }});
  builtinType("Delay", Object.prototype);
  builtinType("Eduction", Object.prototype);
  installMethods(builtinTypes.Eduction.prototype, [
    ["ISequential", 0, null], ["IIterable", 0, null], ["ISeqable", 0, null], ["IReduce", 0, null], ["IPrintWithWriter", 0, null],
    ["IIterable/-iterator", 1, value => {
      const {xform, coll} = eductions.get(value);
      return transformedIterator(xform, [sequenceIterator(coll)]);
    }],
    ["ISeqable/-seq", 1, value => {
      const {xform, coll} = eductions.get(value);
      return seq(sequence(xform, coll));
    }],
    ["IReduce/-reduce", 2, (value, rf) => {
      const {xform, coll} = eductions.get(value);
      // Inner completion flushes buffers without completing the outer reducer.
      return transduce(xform, completing(rf), coll);
    }],
    ["IReduce/-reduce", 3, (value, rf, init) => {
      const {xform, coll} = eductions.get(value);
      return transduce(xform, completing(rf), init, coll);
    }],
    ["IPrintWithWriter/-pr-writer", 3, (value, writer, options) => {
      return prSequentialWriter(writer, printWriter, "(", " ", ")", options, value);
    }]
  ]);
  Object.defineProperties(builtinTypes.Eduction.prototype, Object.getOwnPropertyDescriptors({
    get xform() { return eductions.get(this).xform; },
    set xform(value) { eductions.get(this).xform = value; },
    get coll() { return eductions.get(this).coll; },
    set coll(value) { eductions.get(this).coll = value; },
    indexOf(value, start = 0) { return sequentialIndex(this, value, start, false); },
    lastIndexOf(value, start = count(this)) { return sequentialIndex(this, value, start, true); },
    [Symbol.iterator]() { return es6Iterator(this); }
  }));
  for (const name of ["TransientVector", "TransientArrayMap", "TransientHashMap", "TransientHashSet"]) builtinType(name, transientPrototype);
  Object.defineProperty(builtinTypes.Subvec.prototype, "v", {get() { return subvecSources.get(this); }});
  for (const [Type, store, fields] of [
    [builtinTypes.Delay, delays, {f: "thunk", value: "value"}],
    [builtinTypes.LazySeq, lazyValues, {fn: "thunk", s: "value", __hash: "hash"}]
  ]) for (const [field, key] of Object.entries(fields)) Object.defineProperty(Type.prototype, field, {
    configurable: true, get() { return store.get(this)[key]; }, set(value) { store.get(this)[key] = value; }
  });
  Object.defineProperty(builtinTypes.LazySeq.prototype, "meta", {
    configurable: true, get() { return metadataValues.get(this); }, set(value) { metadataValues.set(this, value); }
  });
  Object.defineProperties(builtinTypes.LazySeq.prototype, Object.getOwnPropertyDescriptors({
    sval() { return deferredValue(lazyValues.get(this), true); },
    equiv(other) { return equalPair(this, other); },
    indexOf(item, start = 0) {
      const length = count(this);
      for (let i = start < 0 ? Math.max(0, length + start) : start; i < length; i++) if (equalPair(nth(this, i), item)) return i;
      return -1;
    },
    lastIndexOf(item, start = count(this)) {
      const length = count(this);
      for (let i = start < 0 ? length + start : Math.min(length - 1, start); i >= 0; i--) if (equalPair(nth(this, i), item)) return i;
      return -1;
    },
    [Symbol.iterator]() { return es6Iterator(this); }
  }));
  for (const name of ["List", "EmptyList", "Cons", "IndexedSeq", "ChunkedSeq", "Repeat", "Range", "IntegerRange"]) {
    Object.defineProperties(builtinTypes[name].prototype, Object.getOwnPropertyDescriptors({
      indexOf: builtinTypes.LazySeq.prototype.indexOf,
      lastIndexOf: builtinTypes.LazySeq.prototype.lastIndexOf
    }));
  }
  for (const name of ["List", "EmptyList", "Cons", "IndexedSeq", "ChunkedSeq", "ChunkedCons", "NodeSeq", "PersistentTreeMapSeq", "PersistentQueue", "Range", "IntegerRange"]) {
    builtinTypes[name].prototype[Symbol.iterator] = builtinTypes.LazySeq.prototype[Symbol.iterator];
  }
  builtinType("ChunkBuffer", Object.prototype);
  builtinType("ArrayChunk", Object.prototype);
  for (const [name, store, fields] of [
    ["ChunkBuffer", chunkBuffers, {buf: "array", end: "end"}],
    ["ArrayChunk", chunks, {arr: "array", off: "offset", end: "end"}]
  ]) for (const [field, key] of Object.entries(fields)) Object.defineProperty(builtinTypes[name].prototype, field, {
    configurable: true, get() { return store.get(this)[key]; }, set(value) { store.get(this)[key] = value; }
  });
  Object.defineProperties(builtinTypes.ChunkBuffer.prototype, Object.getOwnPropertyDescriptors({
    add(value) { return chunkAppend(this, value); },
    chunk() { return chunk(this); }
  }));
  for (const [field, key] of Object.entries({chunk: "chunk", more: "more", __hash: "hash"})) Object.defineProperty(builtinTypes.ChunkedCons.prototype, field, {
    configurable: true, get() { return cells.get(this)[key]; },
    set: field === "__hash" ? function (value) { cells.get(this).chunkedState.hash = value; } : undefined
  });
  Object.defineProperty(builtinTypes.ChunkedCons.prototype, "meta", {get() { return meta(this); }});
  const realized = coreMethod("IPending/-realized?", [1], value => cells.has(value) ? cells.get(value).realized === true : !truth((delays.get(value) || lazyValues.get(value)).thunk));
  builtinTypes.PersistentQueue.EMPTY = queueFromArray([]);
  builtinTypes.PersistentVector.EMPTY = vector();
  builtinTypes.PersistentArrayMap.EMPTY = makeMap([], "array-map");
  builtinTypes.PersistentArrayMap.HASHMAP_THRESHOLD = 8;
  builtinTypes.PersistentHashMap.EMPTY = makeMap([], "hash-map");
  builtinTypes.PersistentHashMap.fromArray = (array, noClone) => {
    const items = noClone ? array : aclone(array), entries = [], length = items.length;
    for (let i = 0; i < length; i += 2) putEntry(entries, items[i], items[i + 1]);
    return makeMap(entries, "hash-map");
  };
  const core = {...builtinTypes, IMultiFn, MultiFn, makeHierarchy, getGlobalHierarchy, isa, parents, ancestors, descendants, derive, underive, bases, supers,
    removeAllMethods: coreMethod("IMultiFn/-reset", [1], resetMulti),
    addMethod: coreMethod("IMultiFn/-add-method", [3], addMethod),
    removeMethod: coreMethod("IMultiFn/-remove-method", [2], removeMethod),
    preferMethod: coreMethod("IMultiFn/-prefer-method", [3], preferMethod),
    getMethod: coreMethod("IMultiFn/-get-method", [2], getMethod),
    methods: coreMethod("IMultiFn/-methods", [1], mf => { demandMulti(mf); return deref(mf.method_table); }),
    prefers: coreMethod("IMultiFn/-prefers", [1], mf => { demandMulti(mf); return deref(mf.prefer_table); }),
    defaultDispatchVal: coreMethod("IMultiFn/-default-dispatch-val", [1], mf => mf.default_dispatch_val),
    dispatchFn: coreMethod("IMultiFn/-dispatch-fn", [1], mf => mf.dispatch_fn),
    IChunk, IChunkedNext, IDrop,
    IEditableCollection, ITransientCollection, ITransientAssociative, ITransientMap, ITransientVector, ITransientSet,
    transient, persistentBang, conjBang, assocBang, dissocBang, disjBang, popBang,
    asTransientProtocol: coreMethod("IEditableCollection/-as-transient", [1], asTransient),
    persistentBangProtocol: coreMethod("ITransientCollection/-persistent!", [1], persistTransient),
    conjBangProtocol: coreMethod("ITransientCollection/-conj!", [2], conjTransient),
    assocBangProtocol: coreMethod("ITransientAssociative/-assoc!", [3], assocTransient),
    assocNBangProtocol: coreMethod("ITransientVector/-assoc-n!", [3], (value, index, item) => assocTransient(value, index, item, true)),
    dissocBangProtocol: coreMethod("ITransientMap/-dissoc!", [2], (value, key) => removeTransient(value, key, "dissoc!")),
    disjoinBangProtocol: coreMethod("ITransientSet/-disjoin!", [2], (value, key) => removeTransient(value, key, "disj!")),
    popBangProtocol: coreMethod("ITransientVector/-pop!", [1], popTransient),
    chunkedNextProtocol: coreMethod("IChunkedNext/-chunked-next", [1], value => cells.get(value)?.kind === "chunked-cons" ? cells.get(value).more ?? null : seq(ownedChunkRest(value))),
    dropProtocol: coreMethod("IDrop/-drop", [2], dropOwned),
    dropFirstProtocol: coreMethod("IChunk/-drop-first", [1], value => { const c = chunks.get(value); if (c.integer ? count(value) <= 1 : count(value) === 0) throw new Error("-drop-first of empty chunk"); const result = arrayChunk(c.array, c.offset + 1, c.end); if (c.integer) chunks.set(result, {...c, start: c.start + c.step, offset: c.offset + 1}); return result; }),
    chunkFirst, chunkRest, chunkNext, chunkCons, ChunkedCons, arrayChunk,
    chunkBuffer, ChunkBuffer, chunkAppend, chunk,
    second: x => first(next(x)), ffirst: x => first(first(x)), nfirst: x => next(first(x)), fnext: x => first(next(x)), nnext: x => next(next(x)),
    reverse: x => reduce((s, value) => conj(s, value), emptyList, x),
    some: (pred, coll) => { for (let s = seq(coll); s !== null; s = next(s)) { const value = pred.call(null, first(s)); if (truth(value)) return value; } return null; },
    notEvery: (pred, coll) => !every(pred, coll), notAny: (pred, coll) => !truth(core.some(pred, coll)), notEmpty: coll => seq(coll) === null ? null : coll,
    run: (proc, coll) => { reduce((_x, value) => proc.call(null, value), null, coll); return null; },
    randomUuid: () => uuid("xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, digit => { const value = Math.floor(Math.random() * 16); return (digit === "x" ? value : (value & 3) | 8).toString(16); })),
    isUri: value => typeof globalThis.goog?.Uri === "function" && value instanceof globalThis.goog.Uri, cat,
    drop, repeatedly, replicate, mapIndexed, mapcat, filter, remove, keep, keepIndexed, takeWhile, dropWhile, takeLast, dropLast, takeNth, splitAt, splitWith, partition, partitionAll, partitionBy, interleave, interpose, dorun, doall, boundedCount, last, butlast, mapv, filterv, IMapEntry, IFind, ISorted, IChunkedSeq,
    isIterable: value => satisfies(IIterable, value),
    writeAll: (writer, ...texts) => { for (const text of texts) write(writer, text); return null; },
    keyProtocol: coreMethod("IMapEntry/-key", [1], value => nth(value, 0)), valProtocol: coreMethod("IMapEntry/-val", [1], value => nth(value, 1)),
    findProtocol: coreMethod("IFind/-find", [2], find),
    sortedSeqProtocol: coreMethod("ISorted/-sorted-seq", [2], (value, ascending) => { const s = truth(ascending) ? seq(value) : rseq(value); return sets.has(value) ? mapSeq(predicates.identity, s) : s; }),
    sortedSeqFromProtocol: coreMethod("ISorted/-sorted-seq-from", [3], sortedSeqFrom),
    entryKeyProtocol: coreMethod("ISorted/-entry-key", [2], (value, entry) => sets.has(value) ? entry : core.keyProtocol(entry)), comparatorProtocol: coreMethod("ISorted/-comparator", [1], value => sortedComparators.get(value)),
    chunkedFirstProtocol: coreMethod("IChunkedSeq/-chunked-first", [1], ownedChunkFirst),
    chunkedRestProtocol: coreMethod("IChunkedSeq/-chunked-rest", [1], ownedChunkRest),
    invokeProtocol: coreMethod("IFn/-invoke", Array.from({length: 22}, (_, i) => i + 1), (value, ...args) => value.call(null, ...args)),...numeric, ...predicates, keyword: keywordCore, symbol: symbolCore, vector, list,
    gensym, macroexpand, macroexpand1, eval: () => { throw new Error("compiler eval service is not installed"); },
    hashMap, arrayMap, sortedMap, sortedMapBy, sortedSet, sortedSetBy, subseq, rsubseq, mkBoundFn, hashSet, dissoc, disj, conj, assoc, seq, seqToMap, first, rest, next, name, namespace, subs, count, nth, get, getIn, assocIn, updateIn, selectKeys, merge, mergeWith, zipmap, contains, find, every, set, isCloneable: value => satisfies(ICloneable, value), equal,
    reduce, completing, transduce, ensureReduced, haltWhen, dedupe, randomSample, add, reduced, isReduced, unreduced, deref, meta, withMeta, varyMeta, resetMeta, alterMeta, atom, reset, swap,
    Atom, atomFactory, Volatile, volatile, volatileFactory, isVolatile, vreset, vresetProtocol, derefProtocol, resetVals, swapVals, compareAndSet, setValidator, getValidator, addWatch, removeWatch, notifyWatches, addWatchProtocol, removeWatchProtocol, resetProtocol, swapProtocol, update,
    IPending, realized, force, isDelay: value => delays.has(value), nthrest, nthnext,
    IDeref, IAtom, IReset, ISwap, IVolatile, IWatchable,
    IReduce, IKVReduce, IVector, IMap, ICollection, IEmptyableCollection, ISet, IStack, IReversible, IComparable, IIterable, reduceKv, subvec, equivMap, SeqIter, RangedIterator,
    reduceProtocol: coreMethod("IReduce/-reduce", [2, 3], function (value, rf, init) { return arguments.length === 2 ? reduce(rf, value) : reduce(rf, init, value); }),
    kvReduceProtocol: coreMethod("IKVReduce/-kv-reduce", [3], (value, rf, init) => reduceKv(rf, init, value)),
    assocProtocol: coreMethod("IAssociative/-assoc", [3], assoc), assocNProtocol: coreMethod("IVector/-assoc-n", [3], assoc),
    dissocProtocol: coreMethod("IMap/-dissoc", [2], dissoc), conjProtocol: coreMethod("ICollection/-conj", [2], conj),
    emptyProtocol: coreMethod("IEmptyableCollection/-empty", [1], empty), disjoinProtocol: coreMethod("ISet/-disjoin", [2], disj),
    peekProtocol: coreMethod("IStack/-peek", [1], peek), popProtocol: coreMethod("IStack/-pop", [1], pop),
    rseqProtocol: coreMethod("IReversible/-rseq", [1], rseq), compareProtocol: coreMethod("IComparable/-compare", [2], compareKeys),
    iteratorProtocol: coreMethod("IIterable/-iterator", [1], iterator),
    hashOrderedColl, hashUnorderedColl, hashDouble, stringHash, rotateLeft, imul: Math.imul,
    compare: compareKeys, INamed, nameProtocol: coreMethod("INamed/-name", [1], value => { if (!builtinCoreImplementation(INamed, value)) throw new Error("No protocol method INamed.-name defined for this value"); return name(value); }), namespaceProtocol: coreMethod("INamed/-namespace", [1], namespace), IHash, IEquiv, hashProtocol: coreMethod("IHash/-hash", [1], hash), equivProtocol: coreMethod("IEquiv/-equiv", [2], equalPair), type: value => value == null ? null : value.constructor,
    Fn, IList, IFn, ISeqable, ISeq, INext, ICounted, IIndexed, ILookup, ISequential, IAssociative, seqProtocol, firstProtocol, restProtocol, nextProtocol, countProtocol, nthProtocol, lookupProtocol, containsProtocol,
    protocolSentinel, IMeta, IWithMeta, IRecord, ICloneable, IPrintWithWriter, IWriter, clone, prWriter, write, flushWriter, satisfies, implements: implementsProtocol, metaProtocol, withMetaProtocol, empty, peek, pop, rseq, keys, vals, repeat, cycle, prStr, str, uuid,
    identical: (a, b) => a === b, isUuid: value => uuids.has(value), isInst: value => value instanceof Date,
    printCore, someVals, requiredGet, minKey, maxKey, reductions, replace, distinct, isDistinct, sort, sortBy,
    someFn, everyPred, memoize, rand, randInt, randNth, parseLong, parseDouble, parseUuid, parseBoolean, isJsSymbol, isSpecialSymbol, groupBy, frequencies, trampoline, comparator, updateVals, updateKeys,
    replOne: null, replTwo: null, replThree: null, replError: null, sourceFile: null,
    commandLineArgs: null, printFn: null, printErrFn: null, printNewline: true, printReadably: true, printLength: null, printLevel: null, printNamespaceMaps: false,
    printMeta: false, printDup: false, flushOnNewline: true, printFnBodies: false,
    setPrintFn, setPrintErrFn, stringPrint, flush, enableConsolePrint, prStrWithOpts, prnStrWithOpts, prSequentialWriter, prSeqWriter, printMetaQ, printForHost,
    println, prn, pr, newline, prnStr, printStr, printlnStr, taggedLiteral, isTaggedLiteral: value => value instanceof TaggedLiteral,
    splitVAt, partitionV, partitionVAll, shuffle, jsIterable, iteration, cljToJs, jsToClj, keyToJs, IEncodeJS, IEncodeClojure,
    munge, mungeStr, demunge, treeSeq, flatten, iter: sequenceIterator, es6Iterator, es6IteratorSeq: iteratorSeq,
    encodeJsProtocol: coreMethod("IEncodeJS/-clj->js", [1], value => value),
    encodeKeyProtocol: coreMethod("IEncodeJS/-key->js", [1], value => value),
    encodeClojureProtocol: coreMethod("IEncodeClojure/-js->clj", [2], value => value),
    Var, test, hash, instMs: value => value.getTime(), isVar: value => vars.has(value),
    ExceptionInfo, exInfo, exData, exMessage, exCause, throwableToMap,
    isInstance: (type, value) => value instanceof type,
    rePattern, reSeq, reFind: (re, text) => reMatch(re, text, false), reMatches: (re, text) => reMatch(re, text, true),
    isRegex: value => value instanceof RegExp, vec, concat, sequence, eduction, apply, applyTo, into,
    cons: (head, tail) => cons(head, tail), listStar, iterate, take, range, toArray, mapSeq,
    arraySeq: function (array, start = 0) { if (arguments.length > 2) return invalidArity(arguments.length); return array != null && start < array.length ? view(array, start, true) : null; },
    array: (...args) => args,
    aget, aset, alength: array => array.length, aclone, makeArray, intoArray, toArray2d, intArray, longArray, doubleArray, objectArray,
    jsObj, jsKeys, jsInvoke, jsDelete, jsIn: (key, object) => key in object,
    uncheckedGet: (object, key) => object[key], uncheckedSet: jsSet,
    checkedAget: aget, checkedAset: aset, checkedAgetStrict, checkedAsetStrict,
    constantly, complement, comp, partial, juxt, fnil};
  const compilerForms = compilerFormBridge();
  // First-party bounded EDN and explicit synchronous input capability runtime.
  Object.assign(core, (rt => {
  const c = rt.core, inspect = rt.inspect;
  const streams = new WeakMap(), tags = new Map(), builtinTags = new Map(), readerMaps = new WeakMap();
  const absent = Object.freeze({}), discarded = Object.freeze({});
  const MAX_INPUT = 1048576, MAX_NODES = 100000, MAX_DEPTH = 32;
  let defaultTag = null, activeDepth = 0;
  function arity(n) { throw new Error('Invalid arity: ' + n); }
  function located(state, message, category = 'reader') {
    const error = new Error(message.slice(0,1024) + ' at ' + state.name + ':' + state.line + ':' + state.column);
    error.source = state.name; error.line = state.line; error.column = state.column;
    error.offset = state.offset; error.category = category;
    return error;
  }
  function fail(state, message, category) { throw located(state, message, category); }
  function duplicate(state, kind, key) {
    const message = kind + ' literal contains duplicate key: ' + c.prStr(key).slice(0,256);
    const error = located(state,message); error.message = message; throw error;
  }
  function stringReader(text, name = '<string-reader>') {
    if (typeof text !== 'string' || typeof name !== 'string') throw new TypeError('Expected input string and source name');
    const stream = Object.freeze({});
    streams.set(stream, {text, position: 0, total: text.length, offset: 0, line: 1, column: 1, cr: false,
      baseLine: 1, name: name.slice(0,256), provider: null, eof: true, busy: false, failed: false, closed: false});
    if (text.length > MAX_INPUT) fail(streams.get(stream), 'Reader input limit exceeded', 'limit');
    return stream;
  }
  function inputProvider(name, provider) {
    if (typeof name !== 'string' || typeof provider !== 'function') throw new TypeError('Expected source name and input provider');
    const stream = stringReader('', name), state = streams.get(stream);
    state.provider = provider; state.eof = false;
    return stream;
  }
  function stateOf(stream) {
    const state = streams.get(stream);
    if (!state) throw new TypeError('Expected input stream');
    if (state.closed) fail(state, 'Input stream is closed', 'host');
    if (state.busy) fail(state, 'input stream is already in use', 'host');
    return state;
  }
  function peek(s) {
    while (s.position === s.text.length && !s.eof) {
      let chunk;
      try { chunk = s.provider(); }
      catch (_) { fail(s, 'Input provider failed', 'host'); }
      if (chunk == null) { s.eof = true; break; }
      if (typeof chunk !== 'string' || chunk.length === 0) fail(s, 'Input provider must return a nonempty string or nil', 'host');
      s.total += chunk.length;
      if (s.total > MAX_INPUT) fail(s, 'Reader input limit exceeded', 'limit');
      // Drop consumed chunks while retaining absolute locations.
      s.text = chunk; s.position = 0; s.baseLine = s.line;
    }
    return s.position === s.text.length ? null : s.text.charAt(s.position);
  }
  function take(s) {
    const ch = peek(s); if (ch == null) return null;
    ++s.position; ++s.offset;
    if (ch === '\r') { ++s.line; s.column = 1; s.cr = true; }
    else if (ch === '\n') { if (!s.cr) ++s.line; s.column = 1; s.cr = false; }
    else if (ch === '\f' && s.cr) { s.column = 1; s.cr = false; }
    else { ++s.column; s.cr = false; }
    return ch;
  }
  const whitespace = ch => ch !== null && /[\s,]/.test(ch);
  const delimiter = ch => ch === null || whitespace(ch) || /[\[\]{}()";^\\]/.test(ch);
  function skip(s) {
    for (;;) {
      while (whitespace(peek(s))) take(s);
      if (peek(s) !== ';') return;
      while (peek(s) !== null && !['\r', '\n'].includes(take(s))) {}
    }
  }
  function token(s, code = false) {
    let result = '';
    while (!delimiter(peek(s)) && !(code && peek(s) !== null && /['@`~]/.test(peek(s)))) result += take(s);
    if (!result || /[@`~]/.test(result)) fail(s, 'Invalid EDN token');
    return result;
  }
  function named(s, text, keyword = false) {
    if (!text || text.startsWith(':') || text.endsWith(':') || /[\s\[\]{}()";^\\]/.test(text)) fail(s, 'Invalid name');
    const slash = text.indexOf('/');
    if (text !== '/' && (slash === 0 || slash === text.length - 1 || text.indexOf('/', slash + 1) !== -1)) fail(s, 'Invalid name');
    return (keyword ? rt.keyword : rt.symbol)(slash > 0 ? text.slice(0, slash) : null, slash > 0 ? text.slice(slash + 1) : text);
  }
  function number(s, text) {
    if (/[NM]$/.test(text)) fail(s, 'Unsupported numeric extension');
    let result;
    const ratio = /^([+-]?\d+)\/(\d+)$/.exec(text);
    if (ratio) { if (Number(ratio[2]) === 0) fail(s, 'Invalid ratio'); result = Number(ratio[1]) / Number(ratio[2]); }
    else if (/^[+-]?0[xX][\da-fA-F]+$/.test(text)) result = (text[0] === '-' ? -1 : 1) * parseInt(text.replace(/^[+-]/, '').slice(2), 16);
    else {
      const radix = /^([+-]?)(\d+)[rR]([\da-zA-Z]+)$/.exec(text);
      if (radix) {
        const base = Number(radix[2]);
        if (base < 2 || base > 36 || [...radix[3].toLowerCase()].some(ch => parseInt(ch, 36) >= base)) fail(s, 'Invalid radix integer');
        result = (radix[1] === '-' ? -1 : 1) * parseInt(radix[3], base);
      } else if (/^[+-]?0\d+$/.test(text)) {
        if (/[89]/.test(text)) fail(s, 'Invalid octal integer');
        result = (text[0] === '-' ? -1 : 1) * parseInt(text.replace(/^[+-]/, ''), 8);
      } else if (/^[+-]?\d+(?:\.\d*)?(?:[eE][+-]?\d+)?$/.test(text)) result = Number(text);
      else fail(s, 'Invalid number');
    }
    if (!Number.isFinite(result)) fail(s, 'Numeric overflow');
    return result;
  }
  function quoted(s, regex = false) {
    let result = ''; take(s);
    for (;;) {
      const ch = take(s);
      if (ch == null) fail(s, 'Unexpected EOF in string');
      if (ch === '"') return result;
      if (ch !== '\\') { result += ch; continue; }
      const escaped = take(s), escapes = {t:'\t', r:'\r', n:'\n', b:'\b', f:'\f', '"':'"', '\\':'\\'};
      if (Object.prototype.hasOwnProperty.call(escapes, escaped)) result += escapes[escaped];
      else if (escaped === 'u') {
        let digits = ''; for (let i = 0; i < 4; ++i) digits += take(s) ?? '';
        if (!/^[\da-fA-F]{4}$/.test(digits)) fail(s, 'Invalid Unicode escape');
        result += String.fromCharCode(parseInt(digits, 16));
      } else if (escaped !== null && /[0-7]/.test(escaped)) {
        let digits = escaped;
        while (digits.length < 3 && peek(s) !== null && /[0-7]/.test(peek(s))) digits += take(s);
        if (parseInt(digits, 8) > 255) fail(s, 'Invalid octal escape');
        result += String.fromCharCode(parseInt(digits, 8));
      } else if (regex && escaped !== null) result += '\\' + escaped;
      else fail(s, 'Unsupported string escape');
    }
  }
  function character(s) {
    take(s); const first = take(s);
    if (first === null) fail(s, 'Unexpected EOF in character');
    let text = first; while (!delimiter(peek(s))) text += take(s);
    const names = {newline:'\n', space:' ', tab:'\t', backspace:'\b', formfeed:'\f', return:'\r'};
    if (text.length === 1) return text;
    if (names[text] !== undefined) return names[text];
    if (/^u[\da-fA-F]{4}$/.test(text)) return String.fromCharCode(parseInt(text.slice(1), 16));
    if (/^o[0-7]{1,3}$/.test(text) && parseInt(text.slice(1), 8) <= 255) return String.fromCharCode(parseInt(text.slice(1), 8));
    fail(s, 'Invalid character');
  }
  function instant(s, value) {
    try { return parseTimestamp(value); } catch (_) { fail(s, 'Invalid instant'); }
  }
  function timestampFields(value) {
    const m = typeof value === 'string' && /^(\d{4})(?:-(\d{2})(?:-(\d{2})(?:T(\d{2})(?::(\d{2})(?::(\d{2})(?:\.(\d+))?)?)?)?)?)?(?:Z|([-+])(\d{2}):(\d{2}))?$/.exec(value);
    if (!m) throw new Error('Unrecognized date/time syntax: ' + c.str(value).slice(0,1024));
    const [year, month, day, hour, minute, second] = [Number(m[1]),Number(m[2] ?? 1),Number(m[3] ?? 1),Number(m[4] ?? 0),Number(m[5] ?? 0),Number(m[6] ?? 0)];
    const days = [31,year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0) ? 29 : 28,31,30,31,30,31,31,30,31,30,31];
    const check = (low, n, high, msg) => { if (!(low <= n && n <= high)) throw new Error(msg + ' Failed:  ' + low + '<=' + n + '<=' + high); return n; };
    check(1,month,12,'timestamp month field must be in range 1..12');
    check(1,day,days[month-1],'timestamp day field must be in range 1..last day in month');
    check(0,hour,23,'timestamp hour field must be in range 0..23');
    check(0,minute,59,'timestamp minute field must be in range 0..59');
    check(0,second,minute === 59 ? 60 : 59,'timestamp second field must be in range 0..60');
    const milliseconds = Number((m[7] ?? '').slice(0,3).padEnd(3,'0'));
    const offset = (m[8] === '-' ? -1 : 1) * (Number(m[9] ?? 0)*60+Number(m[10] ?? 0));
    return [year,month,day,hour,minute,second,milliseconds,offset];
  }
  function parseTimestamp(value) {
    const [year,month,day,hour,minute,second,milliseconds,offset] = timestampFields(value);
    return new Date(Date.UTC(year,month-1,day,hour,minute,second,milliseconds)-offset*60000);
  }
  function jsLiteral(s, value) {
        const data = inspect(value);
        if (data?.kind === 'vector') return rt.compilerForms.jsArray(data.items);
        if (data?.kind === 'map') {
          const pairs = [];
          for (const [key, value] of data.entries) {
            const name = inspect(key);
            if (typeof key !== 'string' && !(name?.kind === 'keyword' && name.namespace === null)) fail(s, 'Invalid JS object literal key');
            pairs.push(typeof key === 'string' ? key : name.name, value);
          }
          return rt.compilerForms.jsObject(pairs);
        }
        fail(s, 'Invalid JS literal');
  }
  function options(value, profile, string = false) {
    const result = {readers: new Map(profile === 'edn' || (profile === 'reader' && value !== undefined) ? [] : tags), fallback: profile === 'edn' || (profile === 'reader' && value !== undefined) ? null : defaultTag,
      eof: string && profile !== 'code' && value === undefined ? null : absent, profile};
    if (value === undefined) return result;
    const map = inspect(value);
    if (map?.kind !== 'map' || c.isRecord(value)) throw new TypeError('Expected reader options map');
    for (const [key, value] of map.entries) {
      const k = inspect(key);
      if (k?.kind !== 'keyword' || k.namespace !== null) throw new TypeError('Invalid reader option');
      if (k.name === 'eof') result.eof = value;
      else if (profile !== 'code' && k.name === 'default') {
        if (value != null && typeof value.call !== 'function') throw new TypeError('Expected default reader function');
        result.fallback = value;
      } else if (profile !== 'code' && k.name === 'readers') {
        if (value == null) continue;
        let entries = typeof value === 'object' && value !== null ? readerMaps.get(value) : undefined;
        if (entries === undefined) {
          const readers = inspect(value);
          if (readers?.kind !== 'map' || c.isRecord(value)) throw new TypeError('Expected readers map');
          entries = readers.entries.map(([tag, handler]) => {
            const n = inspect(tag);
            if (n?.kind !== 'symbol') throw new TypeError('Expected symbol reader key');
            if (handler != null && typeof handler.call !== 'function') throw new TypeError('Expected reader function');
            return [n.spelling, handler];
          });
          // Only immutable privately branded maps have stable validated entries.
          if (rt.ednKind(value) === 'map') readerMaps.set(value,entries);
        }
        for (const [tag, handler] of entries) result.readers.set(tag,handler);
      } else throw new TypeError('Unsupported reader option: ' + k.name);
    }
    return result;
  }
  function validate(s, root, depth, allowBuiltins) {
    const active = new WeakSet(); let nodes = 0;
    function visit(value, d) {
      if (++nodes > MAX_NODES || d + activeDepth > MAX_DEPTH) fail(s, 'EDN result limit exceeded', 'limit');
      if (value === null || ['boolean', 'number', 'string'].includes(typeof value)) return;
      if (typeof value !== 'object') fail(s, 'Unsupported EDN reader result');
      if (active.has(value)) fail(s, 'Cyclic EDN reader result', 'limit');
      active.add(value);
      try {
        const kind = rt.ednKind(value);
        if (allowBuiltins && ['instant', 'uuid'].includes(kind)) return;
        if (kind === 'seq') {
          const seen = new WeakSet(); let seq = c.seq(value);
          while (seq !== null) {
            if (++nodes > MAX_NODES || seen.has(seq)) fail(s, 'EDN sequence limit exceeded', 'limit');
            seen.add(seq); if (rt.ednKind(seq) !== 'seq') fail(s, 'Unsupported EDN sequence tail');
            visit(c.first(seq), d + 1); seq = c.next(seq);
          }
        } else {
          if (!['symbol', 'keyword', 'vector', 'map', 'set'].includes(kind)) fail(s, 'Unsupported EDN reader result');
          const data = inspect(value);
          if (!data || !['symbol', 'keyword', 'vector', 'map', 'set'].includes(data.kind)) fail(s, 'Unsupported EDN reader result');
          if (data.items) for (const item of data.items) visit(item, d + 1);
          if (data.entries) for (const pair of data.entries) { visit(pair[0], d + 1); visit(pair[1], d + 1); }
        }
        const metadata = c.meta(value); if (metadata != null) visit(metadata, d + 1);
      } finally { active.delete(value); }
    }
    visit(root, depth);
  }
  function parse(s, opts) {
    let nodes = 0, anonymousArgs = null, suppressTags = 0;
    const splices = new WeakMap();
    function required(depth) {
      let value; do { value = form(depth); } while (value === discarded);
      if (value === absent) fail(s, 'Unexpected EOF');
      return value;
    }
    function collection(depth, end, kind, namespace) {
      const values = [];
      for (;;) {
        skip(s); const ch = peek(s);
        if (ch == null) fail(s, 'Unexpected EOF in collection');
        if (ch === end) { take(s); break; }
        const value = form(depth + 1);
        if (splices.has(value)) values.push(...splices.get(value));
        else if (value !== discarded) values.push(value);
      }
      if (kind === 'map') {
        if (values.length % 2) fail(s, 'Map requires an even number of forms');
        let seen = c.hashSet();
        for (let i = 0; i < values.length; i += 2) {
          if (namespace !== undefined) {
            const name = inspect(values[i]);
            if (name && ['keyword','symbol'].includes(name.kind) && (name.namespace === null || name.namespace === '_'))
              values[i] = (name.kind === 'keyword' ? rt.keyword : rt.symbol)(name.namespace === '_' ? null : namespace, name.name);
          }
          if (c.contains(seen, values[i])) duplicate(s, 'Map', values[i]);
          seen = c.conj(seen, values[i]);
        }
      }
      if (kind === 'set') {
        let seen = c.hashSet(); for (const value of values) { if (c.contains(seen, value)) duplicate(s, 'Set', value); seen = c.conj(seen, value); }
      }
      return ({vector:rt.vectorFromArray, list:rt.listFromArray, map:rt.mapFromArray, set:rt.setFromArray})[kind](values);
    }
    function metadata(depth) {
      take(s); let metadata = required(depth + 1); const kind = inspect(metadata);
      if (kind?.kind === 'keyword') metadata = c.hashMap(metadata, true);
      else if (typeof metadata === 'string' || kind?.kind === 'symbol') metadata = c.hashMap(rt.keyword(null, 'tag'), metadata);
      else if (kind?.kind !== 'map') fail(s, 'Invalid metadata');
      const value = required(depth + 1);
      try { return c.withMeta(value, c.merge(c.meta(value), metadata)); }
      catch (_) { fail(s, 'Invalid metadata target'); }
    }
    function dispatch(depth) {
      take(s); const ch = peek(s);
      if (ch === '{') { take(s); return collection(depth, '}', 'set'); }
      if (ch === '_') { take(s); required(depth + 1); return discarded; }
      if (ch === '^') return metadata(depth);
      if (ch === '#') {
        take(s); const name = token(s);
        if (name === 'Inf') return Infinity; if (name === '-Inf') return -Infinity; if (name === 'NaN') return NaN;
        fail(s, 'Invalid symbolic value');
      }
      if (ch === ':') {
        take(s); if (peek(s) === ':') fail(s, 'Auto-resolved namespaced maps are unsupported');
        const namespace = token(s); if (namespace.includes('/')) fail(s, 'Invalid map namespace');
        skip(s); if (take(s) !== '{') fail(s, 'Expected namespaced map');
        return collection(depth, '}', 'map', namespace);
      }
      if (opts.profile === 'code' && ch === '?') {
        take(s); const splice = peek(s) === '@'; if (splice) take(s);
        skip(s); if (take(s) !== '(') fail(s, 'Expected reader conditional list');
        let selected = discarded;
        for (;;) {
          skip(s); if (peek(s) === ')') { take(s); break; }
          if (peek(s) === null) fail(s, 'Unexpected EOF in reader conditional');
          const feature = inspect(required(depth + 1));
          if (feature?.kind !== 'keyword') fail(s, 'Expected reader conditional feature keyword');
          const choose = selected === discarded && ['cljs','default'].includes(feature.name);
          if (!choose) ++suppressTags;
          let branch; try { branch = required(depth + 1); } finally { if (!choose) --suppressTags; }
          if (choose) selected = branch;
        }
        if (splice && selected !== discarded) {
          const kind = rt.ednKind(selected);
          if (!['vector','seq'].includes(kind)) fail(s, 'Reader conditional splice requires sequential data');
          const marker = Object.freeze({}); splices.set(marker, c.toArray(selected)); return marker;
        }
        return selected;
      }
      if (opts.profile === 'code' && ch === '(') {
        if (anonymousArgs !== null) fail(s, 'Nested anonymous function shorthand');
        anonymousArgs = {args: new Map(), max: 0}; take(s);
        let body, args;
        try { body = collection(depth, ')', 'list'); args = anonymousArgs; } finally { anonymousArgs = null; }
        const parameters = [];
        for (let n = 1; n <= args.max; ++n) parameters.push(args.args.get(n) ?? c.gensym('p' + n + '__'));
        if (args.args.has(0)) parameters.push(rt.symbol(null, '&'), args.args.get(0));
        return rt.listFromArray([rt.symbol(null, 'fn*'), rt.vectorFromArray(parameters), body]);
      }
      if (opts.profile === 'code' && ch === '"') return c.rePattern(quoted(s, true));
      if (opts.profile === 'code' && ch === "'") { take(s); return rt.listFromArray([rt.symbol(null, 'var'), required(depth + 1)]); }
      if (ch == null || whitespace(ch) || /[=!?('"+\-]/.test(ch)) fail(s, 'Unsupported dispatch syntax');
      const tag = token(s, opts.profile === 'code'); if (tag.startsWith(':')) fail(s, 'Invalid data tag');
      const symbol = named(s, tag), value = required(depth + 1);
      if (suppressTags) return value;
      let handler = opts.readers.get(tag), result;
      if (handler != null) {
        if (handler === builtinTags.get(tag)) {
          if (tag === 'inst') result = instant(s, value);
          else if (tag === 'uuid') { if (typeof value !== 'string' || !/^[\da-fA-F]{8}-(?:[\da-fA-F]{4}-){3}[\da-fA-F]{12}$/.test(value)) fail(s,'Invalid UUID'); result = c.uuid(value); }
          else if (tag === 'js') return jsLiteral(s,value);
          else if (tag === 'queue') { if (inspect(value)?.kind !== 'vector') fail(s,'Queue literal requires a vector'); return c.into(c.PersistentQueue.EMPTY,value); }
        } else result = handler.call(null, value);
      }
      else if (opts.profile === 'code' && tag === 'inst') {
        result = instant(s,value);
      } else if (opts.profile === 'code' && tag === 'uuid') {
        if (typeof value !== 'string' || !/^[\da-fA-F]{8}-(?:[\da-fA-F]{4}-){3}[\da-fA-F]{12}$/.test(value)) fail(s, 'Invalid UUID');
        result = c.uuid(value);
      } else if (opts.fallback != null) result = opts.fallback.call(null, symbol, value);
      else fail(s, 'No reader function for tag ' + tag);
      validate(s, result, depth + 1, opts.profile !== 'edn');
      return result;
    }
    function syntaxQuote(value, gensyms = new Map(), depth = 0) {
      if (depth + activeDepth > MAX_DEPTH) fail(s, 'Syntax quote depth limit exceeded', 'limit');
      const data = inspect(value);
      const sym = name => rt.symbol('cljs.core', name);
      if (data?.kind === 'symbol') {
        let resolved = value;
        if (data.namespace === null) {
          if (data.name.endsWith('#')) { if (!gensyms.has(data.name)) gensyms.set(data.name, c.gensym(data.name.slice(0,-1) + '__')); resolved = gensyms.get(data.name); }
          else if (!['if','do','let*','fn*','loop*','recur','throw','try','new','set!','quote','var','def'].includes(data.name)) {
            const runtimeName = ({'+':'add','-':'subtract','*':'multiply','/':'divide','=':'equal','map':'mapSeq'})[data.name] ?? data.name.replace(/-([a-z])/g, (_,ch) => ch.toUpperCase());
            resolved = rt.symbol(Object.prototype.hasOwnProperty.call(c,runtimeName) || ['let','letfn','fn','loop','defn','when','when-not','and','or','cond','if-let','when-let','doseq','for','lazy-seq'].includes(data.name) ? 'cljs.core' : 'user', data.name);
          }
        }
        return rt.listFromArray([rt.symbol(null,'quote'), resolved]);
      }
      if (!data || !['list','seq','vector','map','set'].includes(data.kind)) return value;
      const items = data.entries ? data.entries.flat() : data.items;
      if ((data.kind === 'list' || data.kind === 'seq') && items.length && inspect(items[0])?.kind === 'symbol') {
        const head = inspect(items[0]).name;
        if (head === 'unquote') { if (items.length !== 2) fail(s,'Invalid unquote'); return items[1]; }
        if (head === 'unquote-splicing') fail(s,'Unquote splicing requires a collection');
      }
      const pieces = items.map(item => {
        const data = inspect(item);
        if ((data?.kind === 'list' || data?.kind === 'seq') && inspect(data.items[0])?.name === 'unquote-splicing') {
          if (data.items.length !== 2) fail(s,'Invalid unquote splice'); return data.items[1];
        }
        return rt.listFromArray([sym('list'), syntaxQuote(item, gensyms, depth + 1)]);
      });
      let result = data.kind === 'list' && items.length === 0 ? rt.listFromArray([sym('list')]) :
        rt.listFromArray([sym('sequence'),rt.listFromArray([sym('seq'),rt.listFromArray([sym('concat'),...pieces])])]);
      if (data.kind === 'vector') result = rt.listFromArray([sym('vec'),result]);
      if (data.kind === 'map' || data.kind === 'set') result = rt.listFromArray([sym('apply'),sym(data.kind === 'map' ? 'hash-map' : 'hash-set'),result]);
      const metadata = c.meta(value);
      if (metadata !== null) result = rt.listFromArray([sym('with-meta'),result,syntaxQuote(metadata,gensyms,depth + 1)]);
      return result;
    }
    function form(depth) {
      if (++nodes > MAX_NODES || depth + activeDepth > MAX_DEPTH) fail(s, 'Reader limit exceeded', 'limit');
      skip(s); const ch = peek(s);
      if (ch === null) return absent;
      if (ch === '"') return quoted(s);
      if (ch === '\\') return character(s);
      if (ch === '#') return dispatch(depth);
      if (ch === '^') return metadata(depth);
      if (ch === '[' || ch === '(' || ch === '{') { take(s); return collection(depth, {'[':']','(':')','{':'}'}[ch], {'[':'vector','(':'list','{':'map'}[ch]); }
      if (opts.profile === 'code' && ch === "'") { take(s); return rt.listFromArray([rt.symbol(null,'quote'), required(depth + 1)]); }
      if (opts.profile === 'code' && ch === '@') { take(s); return rt.listFromArray([rt.symbol('cljs.core','deref'), required(depth + 1)]); }
      if (opts.profile === 'code' && ch === '`') { take(s); return syntaxQuote(required(depth + 1)); }
      if (opts.profile === 'code' && ch === '~') { take(s); const splice = peek(s) === '@'; if (splice) take(s); return rt.listFromArray([rt.symbol(null,splice ? 'unquote-splicing' : 'unquote'), required(depth + 1)]); }
      if (/[\])}'@`~]/.test(ch)) fail(s, 'Unsupported reader syntax');
      const text = token(s, opts.profile === 'code');
      if (opts.profile === 'code' && anonymousArgs !== null && text.startsWith('%')) {
        const n = text === '%&' ? 0 : text === '%' ? 1 : /^%[1-9]\d*$/.test(text) ? Number(text.slice(1)) : -1;
        if (n < 0 || n > MAX_NODES) fail(s, 'Invalid anonymous argument');
        if (!anonymousArgs.args.has(n)) anonymousArgs.args.set(n, c.gensym(n ? 'p' + n + '__' : 'rest__'));
        anonymousArgs.max = Math.max(anonymousArgs.max,n); return anonymousArgs.args.get(n);
      }
      if (opts.profile === 'code' && text.startsWith('::')) return named(s, 'user/' + text.slice(2), true);
      if (text === 'nil') return null; if (text === 'true') return true; if (text === 'false') return false;
      if (/^[+-]?\d/.test(text)) return number(s, text);
      if (text.startsWith(':')) return named(s, text.slice(1), true);
      return named(s, text);
    }
    let value; do { value = form(0); } while (value === discarded);
    if (splices.has(value)) fail(s,'Splicing reader conditional outside collection');
    return value;
  }
  function readWith(profile, args) {
    let stream, option;
    if (args.length === 0) stream = c.readerCurrentInputValue;
    else if (args.length === 1) stream = args[0];
    else if (args.length === 2) [option, stream] = args;
    else if (profile === 'reader' && args.length === 4) {
      stream = args[0]; option = args[3];
    } else if (profile === 'code' && (args.length === 3 || args.length === 4)) {
      stream = args[0]; option = c.hashMap(rt.keyword(null,'eof'), args[1] !== false && args[1] != null ? absent : args[2]);
    } else arity(args.length);
    const opts = options(option, profile), state = stateOf(stream);
    if (profile === 'reader') {
      if (args.length < 2) opts.eof = null;
      else if (args.length === 4) opts.eof = args[1] !== false && args[1] != null ? absent : args[2];
    }
    if (state.failed) { if (opts.eof !== absent) return opts.eof; fail(state, 'EOF while reading'); }
    state.busy = true; ++activeDepth;
    try { const value = parse(state, opts); if (value !== absent) return value; if (opts.eof !== absent) return opts.eof; fail(state, 'EOF while reading'); }
    catch (error) { state.failed = true; throw error; }
    finally { state.busy = false; --activeDepth; }
  }
  function readStringWith(profile, args) {
    if (args.length !== 1 && args.length !== 2) arity(args.length);
    const text = args[args.length - 1];
    if (profile !== 'code' && (text == null || text === false)) return null;
    const opts = options(args.length === 2 ? args[0] : undefined, profile, true);
    const stream = stringReader(text, profile === 'edn' ? '<edn-read-string>' : '<read-string>');
    // readWith receives explicit options so preserve the no-option EDN nil EOF.
    const state = streams.get(stream); ++activeDepth;
    try { const value = parse(state, opts); if (value !== absent) return value; if (opts.eof !== absent) return opts.eof; fail(state,'EOF while reading'); }
    finally { --activeDepth; }
  }
  function readLine(stream = c.readerCurrentInputValue) {
    if (arguments.length > 1) arity(arguments.length);
    const s = stateOf(stream); s.busy = true;
    try {
      let result = '', ch;
      while ((ch = take(s)) !== null) {
        if (ch === '\n') return result;
        if (ch === '\r') { if (peek(s) === '\n') take(s); return result; }
        result += ch;
      }
      return result.length ? result : null;
    } finally { s.busy = false; }
  }
  function tagName(tag) { const name = inspect(tag); if (name?.kind !== 'symbol') throw new TypeError('Expected data reader symbol'); return name.spelling; }
  function register(tag, handler) {
    if (arguments.length !== 2) arity(arguments.length);
    if (handler != null && typeof handler.call !== 'function') throw new TypeError('Expected reader function');
    const name = tagName(tag), previous = tags.get(name) ?? null; tags.set(name, handler); return previous;
  }
  function deregister(tag) { if (arguments.length !== 1) arity(arguments.length); const name = tagName(tag), previous = tags.get(name) ?? null; tags.delete(name); return previous; }
  function registerDefault(handler) { if (arguments.length !== 1) arity(arguments.length); if (handler != null && typeof handler.call !== 'function') throw new TypeError('Expected reader function'); const previous = defaultTag; defaultTag = handler; return previous; }
  function deregisterDefault() { if (arguments.length !== 0) arity(arguments.length); const previous = defaultTag; defaultTag = null; return previous; }
  function withInput(text, thunk) {
    if (arguments.length !== 2) arity(arguments.length);
    const previous = c.readerCurrentInputValue, scoped = stringReader(text, '<with-in-str>');
    c.readerCurrentInputValue = scoped;
    try { return thunk.call(null); } finally {
      const state = streams.get(scoped); state.closed = true; state.provider = null; state.text = '';
      c.readerCurrentInputValue = previous;
    }
  }
  function compilerInputWindow(more = false) {
    const s = stateOf(c.readerCurrentInputValue);
    if ((more || s.position === s.text.length) && !s.eof) {
      const chunk = s.provider();
      if (chunk == null) s.eof = true;
      else {
        if (typeof chunk !== 'string' || !chunk.length) fail(s, 'Invalid input chunk', 'host');
        if (s.text.length + chunk.length > MAX_INPUT) fail(s, 'Reader input limit exceeded', 'limit');
        s.text += chunk;
      }
    }
    return [s.text, s.position, s.eof, s.baseLine];
  }
  function compilerInputConsume(units) {
    const s = stateOf(c.readerCurrentInputValue);
    if (units < 0 || s.position + units > s.text.length) throw new Error('Invalid input cursor');
    for (let i = 0; i < units; ++i) take(s);
  }
  function compilerInputRecover() {
    const s = stateOf(c.readerCurrentInputValue);
    if (s.failed) { compilerInputConsume(s.text.length - s.position); s.failed = false; }
  }
  function compilerInputDiscard() {
    const s = stateOf(c.readerCurrentInputValue);
    compilerInputConsume(s.text.length - s.position); s.failed = false;
  }
  function compilerInputCompact() {
    const s = stateOf(c.readerCurrentInputValue);
    const prefix = s.text.slice(0, s.position);
    const end = Math.max(prefix.lastIndexOf('\n'), prefix.lastIndexOf('\r')) + 1;
    if (end) {
      const lines = (prefix.slice(0, end).match(/\r\n|\r|\n/g) ?? []).length;
      s.baseLine += lines; s.text = s.text.slice(end); s.position -= end;
    }
    s.total = s.text.length;
  }
  function close(stream) { if (arguments.length !== 1) arity(arguments.length); const s = stateOf(stream); s.closed = true; s.provider = null; s.text = ''; return null; }
  const builtinLocation = {name: '<tag-parser>', line: 1, column: 1, offset: 0};
  builtinTags.set('inst', value => instant(builtinLocation,value));
  builtinTags.set('uuid', value => { if (typeof value !== 'string') throw new TypeError('UUID literal requires a string'); return c.uuid(value); });
  builtinTags.set('queue', value => { if (inspect(value)?.kind !== 'vector') throw new TypeError('Queue literal requires a vector'); return c.into(c.PersistentQueue.EMPTY,value); });
  builtinTags.set('js', value => jsLiteral(builtinLocation,value));
  for (const [tag, handler] of builtinTags) tags.set(tag,handler);
  return {ednReadString: (...args) => readStringWith('edn', args), ednRead: (...args) => readWith('edn', args),
    readerReadString: (...args) => readStringWith('reader', args), readerRead: (...args) => readWith('reader', args),
    readString: (...args) => readStringWith('code', args), read: (...args) => readWith('code', args),
    readLine: function () { if (arguments.length) arity(arguments.length); return readLine(); },
    compilerInputWindow, compilerInputConsume, compilerInputRecover, compilerInputDiscard, compilerInputCompact,
    readerStringReader: stringReader, readerInputProvider: inputProvider, readerReadLine: readLine,
    readerParseTimestampFields: function (value) { if (arguments.length !== 1) arity(arguments.length); return rt.vectorFromArray(timestampFields(value)); },
    readerParseTimestamp: function (value) { if (arguments.length !== 1) arity(arguments.length); return parseTimestamp(value); },
    readerInitialTagTable: function () { if (arguments.length) arity(arguments.length); return rt.mapFromArray([...builtinTags].flatMap(([tag,f]) => [rt.symbol(null,tag),f])); },
    readerRegisterTagParser: register, readerDeregisterTagParser: deregister,
    readerRegisterDefaultTagParser: registerDefault, readerDeregisterDefaultTagParser: deregisterDefault,
    readerWithInput: withInput, readerCurrentInputValue: null, readerCurrentInput: () => c.readerCurrentInputValue, readerClose: close};
  })({keyword, symbol, vectorFromArray, listFromArray, mapFromArray, setFromArray, core, inspect, compilerForms, ednKind: value => symbols.has(value) ? 'symbol' : keywords.has(value) ? 'keyword' : vectors.has(value) ? 'vector' : maps.has(value) ? 'map' : sets.has(value) ? 'set' : cells.has(value) || views.has(value) ? 'seq' : uuids.has(value) ? 'uuid' : value instanceof Date ? 'instant' : null}));






  Object.assign(core, {
    configureHostHooks, hostClock: defaultHostClock, hostRandom: defaultHostRandom,
    systemTime, addTap, removeTap, tapGreater, execTapFn: defaultTapSchedule,
    rand: hostRand, randInt: hostRandInt, randNth: hostRandNth,
    shuffle: hostShuffle, randomUuid: hostRandomUuid,
    loadFile: function () { if (arguments.length !== 1) return invalidArity(arguments.length); throw new Error("load-file requires a compiler session and an explicit file provider"); }
  });
  Object.assign(core, {Namespace, createNs, findNs, nsName, theNs, allNs, compilerMacroVar,
    currentNs: createNs(symbol(null, "cljs.user")), assertions: true});
  createNs(symbol(null, "cljs.core"));
  Object.seal(core);
  return Object.freeze({setDeclarationDemand, declarationReceiver, declarationTransaction, applyDeclarationPatches, snapshotDeclarationPatches, restoreDeclarationPatches, compilerExceptionMessage, compilerForms, keyword, symbol, vector, vectorFromArray, listFromArray, mapFromArray, setFromArray, literalSetFromArray, queueFromArray, jsObject, makeVar, definitionVar, makeIFn, protocol, protocolPredicate, protocolMethod, extendType, extendInstance, makeReify, reify, makeType, recordFromMap, field, fnRest, fnApplyTo, setMacroexpandServices, compilerFormBridge, core, inspect, hash});
})()
