(() => {
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
  const mapKinds = new WeakMap();
  const mapEntries = new WeakSet();
  const sets = new WeakMap();
  const cells = new WeakMap();
  const views = new WeakMap();
  const reducedValues = new WeakMap();
  const metadataValues = new WeakMap();
  const uuids = new WeakMap();
  const vars = new WeakMap();
  const atoms = new WeakMap(), volatiles = new WeakMap();
  const queues = new WeakMap();
  const lazyValues = new WeakMap(), delays = new WeakMap();
  const ifnValues = new WeakMap();
  const metaFunctions = new WeakMap();
  const typeNames = new WeakMap();
  const typeFields = new WeakMap(), recordTypes = new WeakMap(), reifyValues = new WeakSet();
  // Extension prototypes are public and mutable; immutable collection payloads
  // and their base callability remain private and frozen.
  const builtinTypes = {};
  function builtinType(name, base) {
    if (!builtinTypes[name]) {
      const Type = name === "Delay" ? Delay : name === "LazySeq" ? LazySeq : name === "MapEntry" ? function (key, value) { return mapEntry(key, value); } : function () {};
      Type.prototype = Object.create(base);
      Object.defineProperty(Type.prototype, "constructor", {value: Type, writable: true, configurable: true});
      builtinTypes[name] = Type;
    }
    return builtinTypes[name];
  }
  function builtinValue(name, base) { return Object.create(builtinType(name, base).prototype); }
  const reducedPrototype = Object.freeze({});
  const sequencePrototype = Object.freeze({toString() { return print(this, true); }});
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
    const value = builtinValue(kind === "indexed" ? "IndexedSeq" : kind === "chunked" ? "ChunkedSeq" : "IndexedSeq", sequencePrototype);
    views.set(value, Object.freeze({storage, offset, indexed, kind, tail}));
    return value;
  }

  function seq(value) {
    if (value == null) return null;
    const method = publicMethod(value, "ISeqable/-seq", 1, true);
    if (method) return method.call(value, value);
    if (value == null || cells.get(value)?.count === 0) return null;
    const lazy = lazyValues.get(value);
    if (lazy) {
      let current = deferredValue(lazy, true);
      // Flatten nested LazySeq results iteratively, retaining the raw final
      // value. CLJS calls seq on that raw value again on each traversal.
      while (lazyValues.has(current)) current = deferredValue(lazyValues.get(current), true);
      lazy.value = current;
      return seq(current);
    }
    const queue = queues.get(value);
    if (queue) return queue.length ? view(queue, 0, false, "queue") : null;
    if (Array.isArray(value)) return value.length ? view(value, 0, true) : null;
    if (cells.has(value) || views.has(value)) return value;
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
    throw new Error("seq not supported on this type");
  }

  function iteratorSeq(iterator) {
    const item = iterator.next();
    if (item.done) return null;
    return installMethods({}, [["ISeqable", 0, null], ["ISeq", 0, null],
      ["ISeqable/-seq", 1, function () { return this; }],
      ["ISeq/-first", 1, () => item.value],
      ["ISeq/-rest", 1, (() => { let tail = missing; return () => {
        if (tail === missing) tail = iteratorSeq(iterator);
        return tail;
      }; })()]]);
  }
  function first(value) {
    if (value == null) return null;
    const method = implementsProtocol(ISeq, value) ? publicMethod(value, "ISeq/-first", 1) : null;
    if (method) return method.call(value, value);
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
    if (start < 0 || end < start || end > count(value)) throw new Error("Index out of bounds");
    if (!predicates.isVector(value)) throw new Error("subvec requires a vector");
    const result = vectorFromArray(Array.from({length: Math.ceil(end - start)}, (_, i) => nth(value, start + i)));
    Object.setPrototypeOf(result, builtinType("Subvec", vectorPrototype).prototype);
    subvecSources.set(result, value);
    return result;
  }
  class SeqIter {
    constructor(value) { this.value = seq(value); }
    hasNext() { return this.value !== null; }
    next() { const item = first(this.value); this.value = next(this.value); return item; }
    remove() { throw new Error("Unsupported operation"); }
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
        putMember(entries, args[i]);
        result = makeSet(entries);
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
      putEntry(entries, key, item);
      for (let i = 0; i < pairs.length; i += 2)
        putEntry(entries, pairs[i], i + 1 < pairs.length ? pairs[i + 1] : null);
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

  function makeMap(entries, kind = entries.length <= 8 ? "array-map" : "hash-map") {
    const value = builtinValue(kind === "hash-map" ? "PersistentHashMap" : kind === "sorted-map" ? "PersistentTreeMap" : "PersistentArrayMap", mapPrototype);
    maps.set(value, Object.freeze(entries));
    mapKinds.set(value, kind);
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
      const index = entryIndex(entries, key, hash(key));
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
      putEntry(entries, core.nthProtocol(pair, 0), core.nthProtocol(pair, 1));
    }
    return updatedMap(value, entries);
  }

  const setPrototype = Object.freeze({
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

  function makeSet(entries) {
    const value = builtinValue("PersistentHashSet", setPrototype);
    sets.set(value, Object.freeze(entries));
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
      const index = entryIndex(entries, key, hash(key));
      if (index >= 0) entries.splice(index, 1);
    }
    return preserveMeta(value, makeSet(entries));
  }

  const vectorPrototype = Object.freeze({
    apply(_self, args) { return this.call(null, ...Array.from(args)); },
    toString() { return print(this, true); },
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
    const method = value == null ? null : publicMethod(value, "ICounted/-count", 1, true, true);
    if (method) return method.call(value, value);
    if (lazyValues.has(value)) return accumulatingCount(value);
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
    if (cell) return cell.count ?? accumulatingCount(value);
    const slice = views.get(value);
    if (slice) return slice.storage.length - slice.offset + count(slice.tail);
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
    const entry = !recordTypes.has(value) && maps.get(value)?.find(entry => equalPair(entry.key, key));
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
      return predicates.isSet(right) && leftSet.length === count(right) &&
        leftSet.every(entry => contains(right, entry.key));
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
    let result = 0;
    for (let i = 0; i < value.length; i++) result = (Math.imul(result, 31) + value.charCodeAt(i)) | 0;
    return result;
  }

  // Internal hash contract is equality consistency, not CLJS hash integers.
  // Weak identity bookkeeping never reads or mutates foreign object fields.
  function hash(value) {
    if (value == null) return 0;
    const method = implementsProtocol(IHash, value) ? builtin(value, "IHash/-hash", 1) : null;
    if (method) return method.call(value, value) | 0;
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
      const lazy = lazyValues.get(value);
      if (lazy?.hash != null) return lazy.hash;
      let result = 1;
      for (let current = seq(value); current !== null; current = next(current))
        result = (Math.imul(result, 31) + hash(first(current))) | 0;
      if (lazy) lazy.hash = result;
      return result;
    }
    const nativeHash = builtin(value, "IHash/-hash", 1);
    if (nativeHash) return nativeHash.call(value, value) | 0;
    if (typeof value === "symbol" || typeof value === "bigint") return stringHash(String(value));
    if (!identityHashes.has(value)) identityHashes.set(value, ++identityCounter);
    return identityHashes.get(value);
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

  const IMapEntry = function () {}, IFind = function () {}, ISorted = function () {}, IChunkedSeq = function () {};
  const IRecord = function () {}, ICloneable = function () {}, IPrintWithWriter = function () {}, IWriter = function () {};
  const IMeta = function () {}, IWithMeta = function () {}, IHash = function () {}, IEquiv = function () {};
  const IPending = function () {}, IDeref = function () {}, IAtom = function () {}, IReset = function () {}, ISwap = function () {}, IVolatile = function () {}, IWatchable = function () {};
  const Fn = function () {}, IList = function () {}, IFn = function () {}, ISeqable = function () {}, ISeq = function () {}, INext = function () {}, ICounted = function () {}, IIndexed = function () {}, ILookup = function () {}, ISequential = function () {}, IAssociative = function () {};
  const IReduce = function () {}, IKVReduce = function () {}, IVector = function () {}, IMap = function () {}, ICollection = function () {}, IEmptyableCollection = function () {}, ISet = function () {}, IStack = function () {}, IReversible = function () {}, IComparable = function () {}, IIterable = function () {};
  const coreProtocolNames = new Map([[IPending, "IPending"], [Fn, "Fn"], [IList, "IList"], [IMapEntry, "IMapEntry"], [IFind, "IFind"], [ISorted, "ISorted"], [IChunkedSeq, "IChunkedSeq"], [IRecord, "IRecord"], [ICloneable, "ICloneable"], [IPrintWithWriter, "IPrintWithWriter"], [IWriter, "IWriter"], [IReduce, "IReduce"], [IKVReduce, "IKVReduce"], [IVector, "IVector"], [IMap, "IMap"], [ICollection, "ICollection"], [IEmptyableCollection, "IEmptyableCollection"], [ISet, "ISet"], [IStack, "IStack"], [IReversible, "IReversible"], [IComparable, "IComparable"], [IIterable, "IIterable"], [IHash, "IHash"], [IEquiv, "IEquiv"], [IFn, "IFn"], [ISeqable, "ISeqable"], [ISeq, "ISeq"], [INext, "INext"], [ICounted, "ICounted"], [IIndexed, "IIndexed"], [ILookup, "ILookup"], [ISequential, "ISequential"], [IAssociative, "IAssociative"], [IMeta, "IMeta"], [IWithMeta, "IWithMeta"], [IDeref, "IDeref"], [IAtom, "IAtom"], [IReset, "IReset"], [ISwap, "ISwap"], [IVolatile, "IVolatile"], [IWatchable, "IWatchable"]]);
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
    if (maps.has(value)) return preserveMeta(value, makeMap([], mapKinds.get(value)));
    if (sets.has(value)) return preserveMeta(value, makeSet([]));
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
    if (!vectors.has(value) && views.get(value)?.kind !== "indexed" && mapKinds.get(value) !== "sorted-map")
      throw new Error("No protocol method IReversible.-rseq defined for this value");
    const items = Object.freeze(toArray(value).reverse());
    return items.length ? view(items, 0, false, "reverse") : null;
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
    if (arguments.length === 1) { item = n; n = Infinity; }
    if (arguments.length < 1 || arguments.length > 2) return invalidArity(arguments.length);
    if (!(n > 0)) return emptyList;
    const value = Object.create(sequencePrototype);
    cells.set(value, Object.freeze({kind: "repeat", head: item, tail: lazySequence(() => repeat(n - 1, item))}));
    return value;
  }
  function cycle(collection) {
    const start = seq(collection);
    if (start === null) return emptyList;
    function step(current) {
      const value = Object.create(sequencePrototype);
      cells.set(value, Object.freeze({kind: "cycle", head: first(current), tail: lazySequence(() => step(next(current) ?? start))}));
      return value;
    }
    return step(start);
  }
  function compareKeys(a, b) {
    if (a === b) return 0;
    const method = builtin(a, "IComparable/-compare", 2);
    if (method) return method.call(a, a, b);
    if (a == null) return -1;
    if (b == null) return 1;
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
    if (typeof a !== typeof b || !["string", "number", "boolean"].includes(typeof a)) throw new Error("Cannot compare different types");
    return a < b ? -1 : a > b ? 1 : 0;
  }
  function sortedMap(...pairs) {
    const result = mapFromArray(pairs, "sorted-map");
    const entries = maps.get(result).slice();
    entries.sort((a, b) => compareKeys(a.key, b.key));
    maps.set(result, Object.freeze(entries));
    return result;
  }
  function metaSupported(value) {
    return typeof value === "function" || !!builtin(value, "IMeta/-meta", 1) || [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, atoms, metaFunctions].some(store => store.has(value)) || reifyValues.has(value);
  }
  function withMetaSupported(value) {
    return !!builtin(value, "IWithMeta/-with-meta", 2) || [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, metaFunctions].some(store => store.has(value)) || reifyValues.has(value);
  }
  function protocolPredicate(protocol, key, value, direct) {
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
    if (protocol === Fn) return metaFunctions.has(value) || vars.has(value);
    if (protocol === IList) return ["list", "cons"].includes(cells.get(value)?.kind);
    if (protocol === IFn) return !!ifnMethods(value) || isIdent(value) || vectors.has(value) || (maps.has(value) && !recordTypes.has(value)) || sets.has(value) || vars.has(value);
    if (protocol === IReduce) return vectors.has(value) || (maps.has(value) && !recordTypes.has(value)) || cells.has(value) || lazyValues.has(value) || (views.has(value) && views.get(value).kind !== "queue");
    if (protocol === IKVReduce) return vectors.has(value) || maps.has(value);
    if (protocol === IPrintWithWriter || protocol === IHash || protocol === IEquiv) return ownedProtocolValue(value);
    if (protocol === IMapEntry) return mapEntries.has(value);
    if (protocol === IFind) return vectors.has(value) || (maps.has(value) && !recordTypes.has(value));
    if (protocol === ISorted) return mapKinds.get(value) === "sorted-map";
    if (protocol === IChunkedSeq) return ["chunked", "chunked-cons"].includes(views.get(value)?.kind);
    if (protocol === ICloneable) return cloneablePayload(value);
    if (protocol === IVector) return vectors.has(value);
    if (protocol === IMap) return maps.has(value);
    if (protocol === ISet) return sets.has(value);
    if (protocol === ICollection) return [vectors, maps, sets, cells, views, queues, lazyValues].some(store => store.has(value));
    if (protocol === IEmptyableCollection) return !recordTypes.has(value) && builtinCoreImplementation(ICollection, value);
    if (protocol === IStack) return vectors.has(value) || cells.has(value) || queues.has(value);
    if (protocol === IReversible) return vectors.has(value) || views.get(value)?.kind === "indexed" || mapKinds.get(value) === "sorted-map";
    if (protocol === IIterable) return builtinCoreImplementation(ICollection, value);
    if (protocol === IComparable) return vectors.has(value);
    if (protocol === ISeqable) return [vectors, maps, sets, cells, views, queues, lazyValues].some(store => store.has(value));
    if (protocol === ISeq || protocol === INext) return cells.has(value) || views.has(value) || queues.has(value) || lazyValues.has(value);
    if (protocol === ISequential) return vectors.has(value) || cells.has(value) || views.has(value) || queues.has(value) || lazyValues.has(value);
    if (protocol === ICounted) return vectors.has(value) || maps.has(value) || sets.has(value) || queues.has(value) || cells.get(value)?.kind === "list" || (views.has(value) && ["indexed", "array-map", "chunked"].includes(views.get(value).kind));
    if (protocol === IIndexed) return vectors.has(value) || views.get(value)?.indexed === true;
    if (protocol === ILookup) return vectors.has(value) || maps.has(value) || sets.has(value);
    if (protocol === IAssociative) return vectors.has(value) || maps.has(value);
    if (protocol === IMeta) return [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, atoms, metaFunctions].some(store => store.has(value)) || reifyValues.has(value);
    if (protocol === IWithMeta) return [symbols, vectors, maps, sets, cells, views, queues, lazyValues, vars, metaFunctions].some(store => store.has(value)) || reifyValues.has(value);
    if (protocol === IAtom) return atoms.has(value) || !!directMethod(value, "IAtom", 0);
    if (protocol === IPending) return delays.has(value) || lazyValues.has(value);
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
      entries.sort((a, b) => compareKeys(a.key, b.key));
      return preserveMeta(original, makeMap(entries, "sorted-map"));
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
    else if (recordTypes.has(value)) {
      // A record copy owns fresh mutable host fields. Copying accessor
      // descriptors would leave their closures pointing at the old instance.
      const Type = recordTypes.get(value);
      result = new Type(...Type.fieldNames.map(name => field(value, name)));
      maps.set(result, maps.get(value)); mapKinds.set(result, mapKinds.get(value));
    }
    else {
      const stores = [symbols, vectors, maps, mapKinds, sets, cells, views, queues, lazyValues, vars];
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
    const value = builtinValue("UUID", uuidPrototype);
    uuids.set(value, text.toLowerCase());
    return value;
  }
  function makeVar(get, symbol, metadata = null) {
    const methods = {};
    for (let n = 0; n <= 20; n++) methods[n] = function (_self, ...args) { return get.call(null).call(null, ...args); };
    methods[21] = function (_self, ...args) { return apply(get.call(null), ...args); };
    const value = makeIFn(methods);
    Object.setPrototypeOf(value, varPrototype);
    vars.set(value, Object.freeze({get, symbol}));
    metadataValues.set(value, metadata);
    return value;
  }
  function definitionVar(get, symbol, metadata = null) {
    const root = get.call(null);
    const base = mapFromArray([keyword(null, "ns"), symbolCore(namespace(symbol)), keyword(null, "name"), symbolCore(name(symbol)),
      keyword(null, "doc"), null, keyword(null, "file"), null, keyword(null, "line"), null, keyword(null, "column"), null, keyword(null, "arglists"), listFromArray([])]);
    return makeVar(get, symbol, assoc(into(base, metadata), keyword(null, "ns"), symbolCore(namespace(symbol)), keyword(null, "name"), symbolCore(name(symbol)), keyword(null, "test"), truth(root) ? root.cljs$lang$test : null));
  }
  function Var(get, symbol, metadata) { const value = makeVar(get, symbol, metadata); metadataValues.set(value, metadata); return value; }
  function test(value) {
    const fn = vars.has(value) ? get(meta(value), keyword(null, "test")) : value == null ? null : value.cljs$lang$test;
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
  function concat(...values) {
    function step(index, current) {
      return lazySequence(() => {
        let items = seq(current);
        while (items === null && index < values.length) items = seq(values[index++]);
        if (items === null) return null;
        const result = Object.create(sequencePrototype);
        cells.set(result, Object.freeze({kind: "cons", head: first(items), tail: step(index, rest(items))}));
        return result;
      });
    }
    return step(0, null);
  }
  function sequence(value) { return predicates.isSeq(value) ? value : seq(value) ?? emptyList; }
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
    protocolNames.set(value, name); protocolValues.set(name, value);
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
    if (value == null) return undefined;
    const method = value[protocolSlot(key, arity)];
    return arity === 0 ? (method === protocolSentinel ? method : undefined) : method;
  }
  const protocolStores = [keywords, symbols, vectors, maps, sets, cells, views, queues, lazyValues, delays, vars, atoms, volatiles, reducedValues, metaFunctions];
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
    protocolKeys.set(dispatch, key); protocolFunctions.set(key, metadataName); protocolVariables.set(metadataName, dispatch);
    protocolGetters.set(key, getter ?? (() => protocolVariables.get(metadataName)));
    return dispatch;
  }
  function extendType(target, methods) {
    let result = null;
    if (typeof target === "string") {
      const grouped = new Map();
      for (const [key, arity, method] of methods) {
        if (arity === 0) {
          const value = protocolValues.get(key) ?? coreProtocolsByName.get(key);
          if (value) result = value[target] = true;
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
        if (dispatch) dispatch[target] = implementation;
        if (!protocolFunctions.has(key)) {
          if (!nativeMethods.has(key)) nativeMethods.set(key, new Map());
          nativeMethods.get(key).set(target, implementations);
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
    const implementations = protocolImplementations.get(value) ?? new Map(), calls = {}, invokes = {}, objectVariadics = new Map();
    for (const [dispatch, arity, method, invoke = method, variadic] of methods) {
      const key = typeof dispatch === "string" ? dispatch : protocolKeys.get(dispatch);
      if (!implementations.has(key)) implementations.set(key, new Map());
      if (variadic != null && key.startsWith("Object/")) objectVariadics.set(key, [variadic, method]);
      else implementations.get(key).set(arity, method);
      if (!key.startsWith("Object/") && key !== "IFn/-invoke") Object.defineProperty(value, protocolSlot(key, arity), {value: arity === 0 ? protocolSentinel : method, writable: true, configurable: true});
      if (key === "IFn/-invoke") { calls[arity - 1] = method; invokes[arity - 1] = invoke; }
    }
    for (const [key, entries] of implementations) if (key.startsWith("Object/")) {
      Object.defineProperty(value, mungeMember(key.slice(7)), {value: function (...args) {
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
      Object.defineProperties(value, {call: {value: ifnPrototype.call, writable: true, configurable: true}, apply: {value: ifnPrototype.apply, writable: true, configurable: true}});
      for (const [arity, method] of Object.entries(invokes)) value["cljs$core$IFn$_invoke$arity$" + arity] = function (...args) { return method.call(this, this, ...args); };
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
        const readable = truth(options == null ? core.printReadably : get(options, keyword(null, "readably")));
        return write(writer, "#" + (typeNames.get(Type) ?? "").replaceAll("/", ".") + "{" +
          recordEntries(this, Type).map(e => print(nth(e, 0), readable) + " " + print(nth(e, 1), readable)).join(", ") + "}");
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
  function iterate(fn, seed) {
    function node(previous, value, realized) {
      const result = Object.create(sequencePrototype);
      let tail;
      cells.set(result, Object.freeze({kind: "iterate",
        get head() {
          if (!realized) { value = fn.call(null, previous); realized = true; }
          return value;
        },
        get tail() {
          // Iterate.next realizes this seed, not the following seed. Bounded
          // argument counting must therefore never invoke f one step early.
          return tail || (tail = node(first(result), undefined, false));
        }
      }));
      return result;
    }
    return node(null, seed, true);
  }
  function take(n, coll) {
    return lazySequence(() => n > 0 && seq(coll) !== null ? cons(first(coll), take(n - 1, rest(coll))) : null);
  }
  function mapSeq(fn, ...colls) {
    if (colls.length === 0) return invalidArity(1);
    return lazySequence(() => {
      const sources = colls.map(seq);
      if (sources.some(value => value === null)) return null;
      const slice = views.get(sources[0]);
      if (sources.length === 1 && ["chunked", "chunked-cons"].includes(slice?.kind)) {
        const end = Math.min(slice.storage.length, slice.offset + 32 - slice.offset % 32);
        const items = [];
        for (let i = slice.offset; i < end; i++) items.push(fn.call(null, slice.storage[i]));
        const tail = end < slice.storage.length ? view(slice.storage, end, slice.indexed, slice.kind, slice.tail) : slice.tail;
        return view(Object.freeze(items), 0, false, "chunked-cons", mapSeq(fn, tail));
      }
      return cons(fn.call(null, ...sources.map(first)), mapSeq(fn, ...sources.map(rest)));
    });
  }
  function range(start, end, step = 1) {
    if (arguments.length === 0) return iterate(x => x + 1, 0);
    if (arguments.length === 1) { end = start; start = 0; }
    if (arguments.length > 3) return invalidArity(arguments.length);
    if (!(step > 0 ? start < end : start > end)) return emptyList;
    const value = Object.create(sequencePrototype);
    cells.set(value, Object.freeze({kind: "range", head: start, tail: lazySequence(() => range(start + step, end, step))}));
    return value;
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
  function partial(fn, ...prefix) {
    if (!arguments.length) { fn = undefined; prefix = [undefined, undefined, undefined]; }
    if (!prefix.length) return fn;
    if (prefix.length > 3) return callable(0, {}, args => apply(fn, ...prefix.slice(0, 3), concat(prefix.slice(3), args)));
    const run = (...args) => fn.call(null, ...prefix, ...args);
    return callable(3, {0: () => run(), 1: x => run(x), 2: (x, y) => run(x, y), 3: (x, y, z) => run(x, y, z)},
      (x, y, z, tail) => apply(fn, ...prefix, x, y, z, tail));
  }
  function juxt(...fns) {
    if (!fns.length) fns = [undefined, undefined, undefined];
    const run = (args, tail) => vectorFromArray(fns.map(fn => directOrApply(fn, args, tail)));
    return callable(3, {0: () => run([], missing), 1: x => run([x], missing), 2: (x, y) => run([x, y], missing), 3: (x, y, z) => run([x, y, z], missing)},
      (x, y, z, tail) => run([x, y, z], tail));
  }
  function fnil(fn, ...defaults) {
    if (defaults.length < 1 || defaults.length > 3) return invalidArity(arguments.length);
    const patch = args => args.map((x, i) => i < defaults.length && x == null ? defaults[i] : x);
    const run = (...args) => fn.call(null, ...patch(args));
    const methods = {2: (x, y) => run(x, y), 3: (x, y, z) => run(x, y, z)};
    if (defaults.length === 1) methods[1] = x => run(x);
    return callable(3, methods, (x, y, z, tail) => apply(fn, ...patch([x, y, z]), tail));
  }
  function intoReducer(to) {
    // Editable built-ins use construction operations rather than persistent
    // protocol overrides, matching CLJS's transient path without exposing it.
    if (vectors.has(to) && !mapEntries.has(to)) return (acc, item) => makeVector([...vectors.get(acc), item]);
    if (sets.has(to)) return (acc, item) => { const entries = sets.get(acc).slice(); putMember(entries, item); return makeSet(entries); };
    if (maps.has(to) && !recordTypes.has(to) && mapKinds.get(to) !== "sorted-map") {
      const associate = (acc, key, value) => {
        const entries = maps.get(acc).slice(); putEntry(entries, key, value);
        return makeMap(entries, mapKinds.get(acc) === "hash-map" || entries.length > 8 ? "hash-map" : "array-map");
      };
      return (acc, item) => {
        if (predicates.isMapEntry(item)) return associate(acc, core.keyProtocol(item), core.valProtocol(item));
        if (predicates.isVector(item)) return associate(acc, item.call(null, 0), item.call(null, 1));
        for (let current = seq(item); current != null; current = next(current)) {
          const entry = first(current);
          if (!truth(entry)) break;
          acc = associate(acc, core.keyProtocol(entry), core.valProtocol(entry));
        }
        return acc;
      };
    }
    return to == null ? conj : core.conjProtocol;
  }
  function editableCollection(value) {
    return vectors.has(value) && !mapEntries.has(value) || sets.has(value) || maps.has(value) && !recordTypes.has(value) && mapKinds.get(value) !== "sorted-map";
  }
  function editableInitial(value) {
    if (vectors.has(value)) return makeVector(vectors.get(value).slice());
    if (sets.has(value)) return makeSet(sets.get(value).slice());
    return makeMap(maps.get(value).slice(), mapKinds.get(value));
  }
  function into(to, from, source) {
    switch (arguments.length) {
      case 0: return vector();
      case 1: return to;
      case 2: {
        const editable = editableCollection(to);
        const result = reduce(intoReducer(to), editable ? editableInitial(to) : to, from);
        return editable ? withMeta(result, meta(to)) : result;
      }
      case 3: {
        const editable = editableCollection(to), metadata = editable ? meta(to) : null;
        const add = intoReducer(to);
        const rf = !editable ? conj : function (...args) {
          if (args.length === 1) return withMeta(args[0], metadata);
          if (args.length === 2) return add(args[0], args[1]);
          return invalidArity(args.length);
        };
        const transformed = from.call(null, rf);
        return transformed.call(null, reduce(transformed, editable ? editableInitial(to) : to, source));
      }
      default: return invalidArity(arguments.length);
    }
  }
  // Match the CLJS constructor: preserve the supplied message/data/cause values
  // and Error identity, including when callers construct ExceptionInfo directly.
  function ExceptionInfo(message, data, cause) {
    const error = new Error(message);
    const target = this == null ? globalThis : Object(this);
    target.message = message;
    target.data = data;
    target.cause = cause;
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
  function printItems(values) {
    const saved = core.printReadably;
    core.printReadably = false;
    try {
      if (core.printFn == null) throw new Error("No *print-fn* fn set for evaluation environment");
      core.printFn.call(null, values.map(value => print(value, false)).join(" "));
      return null;
    } finally { core.printReadably = saved; }
  }
  function print(value, readable) {
    const writerMethod = value == null ? null : publicMethod(value, "IPrintWithWriter/-pr-writer", 3);
    if (writerMethod) {
      let result = "";
      const writer = installMethods({}, [["IWriter", 0, null], ["IWriter/-write", 2, (_self, text) => { result += text; return null; }], ["IWriter/-flush", 1, () => null]]);
      writerMethod.call(value, value, writer, null);
      return result;
    }
    if (value == null) return "nil";
    if (typeof value === "string") return readable ? JSON.stringify(value) : value;
    if (typeof value === "number") return Number.isNaN(value) ? "##NaN" : value === Infinity ? "##Inf" : value === -Infinity ? "##-Inf" : Object.is(value, -0) ? "-0.0" : String(value);
    if (typeof value === "boolean") return String(value);
    if (typeof value === "function" && value.cljs$lang$type && typeNames.has(value)) return typeNames.get(value);
    if (value instanceof ExceptionInfo) return value.toString();
    if (atoms.has(value) || volatiles.has(value)) return "#object[cljs.core." + (atoms.has(value) ? "Atom" : "Volatile") + " {:val " + print(value.state, readable) + "}]";
    if (delays.has(value)) {
      const state = delays.get(value);
      return "#object[cljs.core.Delay {:status " + (state.thunk == null ? ":ready" : ":pending") + ", :val " + print(state.value, readable) + "}]";
    }
    if (value instanceof Error) return "#object[" + (value.constructor?.name || "Object") + " " + String(value) + "]";
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
  function prStr(...values) { return values.map(value => print(value, truth(core.printReadably))).join(" "); }
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
  const prWriter = coreMethod("IPrintWithWriter/-pr-writer", [3], (value, writer, options) => write(writer, print(value, truth(options == null ? core.printReadably : get(options, keyword(null, "readably"))))));
  const clone = coreMethod("ICloneable/-clone", [1], value => {
    if (!cloneablePayload(value)) throw new Error("No protocol method ICloneable.-clone defined for this value");
    const result = Object.create(Object.getPrototypeOf(value));
    for (const store of [...protocolStores, metadataValues, mapKinds]) if (store.has(value)) store.set(result, store.get(value));
    if (mapEntries.has(value)) mapEntries.add(result);
    return result;
  });
  const seqProtocol = coreMethod("ISeqable/-seq", [1], seq), firstProtocol = coreMethod("ISeq/-first", [1], first), restProtocol = coreMethod("ISeq/-rest", [1], rest), nextProtocol = coreMethod("INext/-next", [1], next), countProtocol = coreMethod("ICounted/-count", [1], count), nthProtocol = coreMethod("IIndexed/-nth", [2, 3], nth), lookupProtocol = coreMethod("ILookup/-lookup", [2, 3], get), containsProtocol = coreMethod("IAssociative/-contains-key?", [2], contains);
  for (const [names, base] of [
    [["PersistentVector", "Subvec", "MapEntry"], vectorPrototype],
    [["PersistentArrayMap", "PersistentHashMap", "PersistentTreeMap"], mapPrototype],
    [["PersistentHashSet"], setPrototype], [["List", "EmptyList", "Cons", "IndexedSeq", "LazySeq", "PersistentQueue"], sequencePrototype],
    [["Keyword"], keywordPrototype], [["Symbol"], symbolPrototype], [["Reduced"], reducedPrototype], [["UUID"], uuidPrototype]
  ]) for (const name of names) builtinType(name, base);
  builtinType("Delay", Object.prototype);
  for (const [Type, store, fields] of [
    [builtinTypes.Delay, delays, {f: "thunk", value: "value"}],
    [builtinTypes.LazySeq, lazyValues, {fn: "thunk", s: "value", __hash: "hash"}]
  ]) for (const [field, key] of Object.entries(fields)) Object.defineProperty(Type.prototype, field, {
    configurable: true, get() { return store.get(this)[key]; }, set(value) { store.get(this)[key] = value; }
  });
  Object.defineProperty(builtinTypes.LazySeq.prototype, "meta", {
    configurable: true, get() { return metadataValues.get(this); }, set(value) { metadataValues.set(this, value); }
  });
  Object.assign(builtinTypes.LazySeq.prototype, {
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
    [Symbol.iterator]() {
      let value = this, started = false;
      return {next() {
        value = started ? next(value) : seq(value); started = true;
        return value == null ? {done: true} : {value: first(value), done: false};
      }};
    }
  });
  const realized = coreMethod("IPending/-realized?", [1], value => !truth((delays.get(value) || lazyValues.get(value)).thunk));
  builtinTypes.PersistentQueue.EMPTY = queueFromArray([]);
  const core = Object.seal({...builtinTypes, IMapEntry, IFind, ISorted, IChunkedSeq,
    isIterable: value => satisfies(IIterable, value),
    writeAll: (writer, ...texts) => { for (const text of texts) write(writer, text); return null; },
    keyProtocol: coreMethod("IMapEntry/-key", [1], value => nth(value, 0)), valProtocol: coreMethod("IMapEntry/-val", [1], value => nth(value, 1)),
    findProtocol: coreMethod("IFind/-find", [2], find),
    sortedSeqProtocol: coreMethod("ISorted/-sorted-seq", [2], (value, ascending) => ascending ? seq(value) : rseq(value)),
    sortedSeqFromProtocol: coreMethod("ISorted/-sorted-seq-from", [3], (value, key, ascending) => seq(toArray(ascending ? seq(value) : rseq(value)).filter(entry => (ascending ? 1 : -1) * compareKeys(nth(entry, 0), key) >= 0))),
    entryKeyProtocol: coreMethod("ISorted/-entry-key", [2], (_value, entry) => nth(entry, 0)), comparatorProtocol: coreMethod("ISorted/-comparator", [1], () => compareKeys),
    chunkedFirstProtocol: coreMethod("IChunkedSeq/-chunked-first", [1], value => vectorFromArray(toArray(value).slice(0, 32))),
    chunkedRestProtocol: coreMethod("IChunkedSeq/-chunked-rest", [1], value => seq(toArray(value).slice(32)) ?? emptyList),
    invokeProtocol: coreMethod("IFn/-invoke", Array.from({length: 22}, (_, i) => i + 1), (value, ...args) => value.call(null, ...args)),...numeric, ...predicates, keyword: keywordCore, symbol: symbolCore, vector, list,
    hashMap, arrayMap, sortedMap, hashSet, dissoc, disj, conj, assoc, seq, seqToMap, first, rest, next, name, namespace, count, nth, get, getIn, assocIn, updateIn, selectKeys, merge, mergeWith, zipmap, contains, find, every, set, isCloneable: value => satisfies(ICloneable, value), equal,
    reduce, add, reduced, isReduced, unreduced, deref, meta, withMeta, varyMeta, resetMeta, alterMeta, atom, reset, swap,
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
    compare: compareKeys, IHash, IEquiv, hashProtocol: coreMethod("IHash/-hash", [1], hash), equivProtocol: coreMethod("IEquiv/-equiv", [2], equalPair), type: value => value == null ? null : value.constructor,
    Fn, IList, IFn, ISeqable, ISeq, INext, ICounted, IIndexed, ILookup, ISequential, IAssociative, seqProtocol, firstProtocol, restProtocol, nextProtocol, countProtocol, nthProtocol, lookupProtocol, containsProtocol,
    protocolSentinel, IMeta, IWithMeta, IRecord, ICloneable, IPrintWithWriter, IWriter, clone, prWriter, write, flushWriter, satisfies, implements: implementsProtocol, metaProtocol, withMetaProtocol, empty, peek, pop, rseq, keys, vals, repeat, cycle, prStr, str, uuid,
    identical: (a, b) => a === b, isUuid: value => uuids.has(value), isInst: value => value instanceof Date,
    printCore, printFn: null, printNewline: true, printReadably: true,
    Var, test, hash, instMs: value => value.getTime(), isVar: value => vars.has(value),
    ExceptionInfo, exInfo, exData, exMessage, exCause, throwableToMap,
    isInstance: (type, value) => value instanceof type,
    rePattern, reSeq, reFind: (re, text) => reMatch(re, text, false), reMatches: (re, text) => reMatch(re, text, true),
    isRegex: value => value instanceof RegExp, vec, concat, sequence, apply, applyTo, into,
    cons: (head, tail) => cons(head, tail), listStar, iterate, take, range, toArray, mapSeq,
    arraySeq: function (array, start = 0) { if (arguments.length > 2) return invalidArity(arguments.length); return array != null && start < array.length ? view(array, start, true) : null; },
    array: (...args) => args,
    aget, aset, alength: array => array.length, aclone, makeArray, intoArray,
    jsObj, jsKeys, jsInvoke, jsDelete, jsIn: (key, object) => key in object,
    uncheckedGet: (object, key) => object[key], uncheckedSet: jsSet,
    checkedAget: aget, checkedAset: aset, checkedAgetStrict, checkedAsetStrict,
    constantly, complement, comp, partial, juxt, fnil});
  return Object.freeze({keyword, symbol, vector, vectorFromArray, listFromArray, mapFromArray, setFromArray, queueFromArray, jsObject, makeVar, definitionVar, makeIFn, protocol, protocolPredicate, protocolMethod, extendType, extendInstance, makeReify, reify, makeType, recordFromMap, field, fnRest, fnApplyTo, core, inspect, hash});
})();
  return (rt.core.reduce).call(null, rt.core.add, (rt.core.take).call(null, 4, ((function l0(l1) {
    return new rt.core.LazySeq(null, (function() {
      return (rt.core.cons).call(null, l1, (l0).call(null, (rt.core.inc).call(null, l1)));
    }), null, null);
  })).call(null, 0)));
})()
