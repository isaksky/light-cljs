function regexpWait(dep) {
  // RegExp objects across dependency waits (light-cljs-942): `dep` is a
  // host effect the test answers with "D", in place or after the job's
  // whole stack was saved as a continuation and restored in another heap
  // or installed over a reused worker's image (snapshot_tests.goose
  // regexp_saved_frames). A literal's object and a run-time pattern's,
  // whose program rows are job code rows, are made before the first wait
  // and read after the last; the RegExp constructor's frame waits in every
  // phase whose value can run JavaScript (IsRegExp's @@match read, the
  // identity rule's `constructor`, the pattern's `source` and `flags`,
  // their ToString conversions, newTarget's `prototype`), the flags
  // getter's frame in two flag reads and toString's in its `source` read
  // and its flags conversion; exec's frame (light-cljs-933) in its
  // argument's conversion and its lastIndex's, test's in its argument's
  // conversion, its `exec` getter and a user exec; a run-time pattern and
  // a literal follow the last wait. 61 waits, the later frames' listed
  // below; completes with REGEXP_RESULT there.
  let log = '';
  const literal = /a(b)?c/gi;
  literal.lastIndex = 3;
  const made = new RegExp('x' + 'y*', 'my');
  const pattern = {
    get [Symbol.match]() { log += 'm' + dep(); return true; },
    get constructor() { log += 'c' + dep(); return Object; },
    get source() { log += 's' + dep(); return { toString() { log += 'x' + dep(); return 'q+'; } }; },
    get flags() { log += 'f' + dep(); return { toString() { log += 'y' + dep(); return 'gu'; } }; }
  };
  const called = RegExp(pattern);
  const B = RegExp.bind(null);
  Object.defineProperty(B, 'prototype', { get() { log += 'p' + dep(); return RegExp.prototype; } });
  const constructed = Reflect.construct(RegExp, [made, 'd'], B);
  const flags = Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get.call({
    get global() { log += 'g' + dep(); return true; },
    get sticky() { log += 'k' + dep(); return 1; }
  });
  const text = RegExp.prototype.toString.call({
    get source() { log += 'S' + dep(); return 'src'; },
    flags: { toString() { log += 'F' + dep(); return 'fl'; } }
  });
  const found = made.exec({ toString() { log += 'T' + dep(); return 'axyyb'; } });
  const counted = /(?<d>\d)/g;
  counted.lastIndex = { valueOf() { log += 'V' + dep(); return 2; } };
  const digit = counted.exec('1a2b3');
  const tested = /c$/.test({ toString() { log += 'U' + dep(); return 'abc'; } });
  const viaGetter = /q/;
  Object.defineProperty(viaGetter, 'exec', { get() { log += 'E' + dep(); return undefined; } });
  const user = /z/;
  user.exec = function (s) { log += 'R' + dep(); return s === 'w' ? {} : null; };
  const answers = [viaGetter.test('aq'), user.test('w')].join('/');
  // replace, replaceAll and @@replace (light-cljs-934): String's frame in
  // IsRegExp's @@match read, the @@replace getter, the search value's
  // conversion and a replacer; @@replace's in its argument's conversion,
  // `flags`, a user exec, a result's `index`, a `$<name>` read through a
  // groups getter, a replacer, and an empty match's lastIndex conversion.
  const search = { get [Symbol.match]() { log += 'M' + dep(); return false; }, get [Symbol.replace]() { log += 'P' + dep(); return undefined; }, toString() { log += 'Q' + dep(); return '-'; } };
  const dashes = 'a-b'.replaceAll(search, () => { log += 'R' + dep(); return '+'; });
  const digits = /(\d)(?<n>x)?/g;
  Object.defineProperty(digits, 'flags', { get() { log += 'G' + dep(); return 'g'; } });
  let execs = 0;
  digits.exec = function (s) {
    const m = RegExp.prototype.exec.call(this, s);
    if (execs++ === 0) {
      log += 'X' + dep();
      Object.defineProperty(m, 'index', { get() { log += 'I' + dep(); return 1; } });
      m.groups = { get n() { log += 'N' + dep(); return 'n'; } };
    }
    return m;
  };
  const swapped = digits[Symbol.replace]({ toString() { log += 'A' + dep(); return 'a1b2'; } }, '[$1$<n>]');
  const replaced = 'xy'.replace(/y/, () => { log += 'C' + dep(); return 'c'; });
  const empty = /(?:)/g;
  let advanced = 0;
  empty.exec = function (s) {
    const m = RegExp.prototype.exec.call(this, s);
    if (m && advanced++ === 0) { const at = this.lastIndex; this.lastIndex = { valueOf() { log += 'L' + dep(); return at; } }; }
    return m;
  };
  const spaced = 'ab'.replace(empty, '.');
  // match, matchAll, search and split (light-cljs-937): String's matchAll
  // frame in IsRegExp's @@match read, `flags`, the @@matchAll getter, a
  // user @@matchAll and its receiver's conversion; @@match's in its
  // argument's conversion, `flags`, lastIndex's reset, a user exec, a
  // result's "0", an empty match's lastIndex conversion and its write;
  // @@search's lastIndex reads and writes and a result's `index`;
  // @@matchAll's `constructor` and @@species getters, the species
  // constructor, the lastIndex conversion and the matcher's lastIndex
  // write; the iterator's next in its `exec` getter, a user exec, a
  // result's "0", an empty match's lastIndex conversion and write; and
  // @@split's limit conversion, the splitter's lastIndex writes and read,
  // a result's `length` and a capture.
  const lookalike = {
    get [Symbol.match]() { log += 'a' + dep(); return true; },
    get flags() { log += 'b' + dep(); return 'g'; },
    get [Symbol.matchAll]() { log += 'd' + dep(); return function (s) { log += 'e' + dep(); return s.length; }; }
  };
  const delegated = 'xyz'.matchAll(lookalike);
  const converted = [...String.prototype.matchAll.call({ toString() { log += 'h' + dep(); return 'abab'; } }, 'b')].map(m => m.index).join('');
  let at = 0, calls = 0;
  const matcher = {
    get flags() { log += 'i' + dep(); return 'g'; },
    set lastIndex(v) { log += 'j' + dep(); at = v; },
    get lastIndex() { return { valueOf() { log += 'l' + dep(); return at; } }; },
    exec(s) { log += 'n' + dep(); return calls++ < 1 ? { get 0() { log += 'o' + dep(); return ''; } } : null; }
  };
  const matched = RegExp.prototype[Symbol.match].call(matcher, { toString() { log += 'q' + dep(); return 'st'; } }).length;
  let saved = 3;
  const searcher = {
    get lastIndex() { log += 'r' + dep(); return saved; }, set lastIndex(v) { log += 't' + dep(); saved = v; },
    exec() { saved = 1; return { get index() { log += 'u' + dep(); return 2; } }; }
  };
  const searched = RegExp.prototype[Symbol.search].call(searcher, 'abc');
  let position = 0, nexts = 0;
  const Species = function (r, f) {
    log += 'v' + dep();
    Object.defineProperty(this, 'lastIndex', { get() { return { valueOf() { log += 'O' + dep(); return position; } }; }, set(v) { log += 'K' + dep(); position = v; } });
    Object.defineProperty(this, 'exec', { get() { log += 'W' + dep(); return function (s) { log += 'w' + dep(); return nexts++ < 1 ? { get 0() { log += 'z' + dep(); return ''; }, index: position } : null; }; } });
  };
  const iterable = {
    get constructor() { log += 'B' + dep(); return { get [Symbol.species]() { log += 'H' + dep(); return Species; } }; },
    flags: 'g', lastIndex: { valueOf() { log += 'J' + dep(); return 1; } }
  };
  const step = RegExp.prototype[Symbol.matchAll].call(iterable, 'pq').next();
  let cut = 0, splits = 0;
  const splitter = {
    set lastIndex(v) { log += 'Y' + dep(); cut = v; },
    get lastIndex() { log += 'Z' + dep(); return cut + 1; },
    exec(s) { return splits++ < 1 ? { get length() { log += '1' + dep(); return 2; }, get 1() { log += '2' + dep(); return 'C'; } } : null; }
  };
  const pieces = RegExp.prototype[Symbol.split].call({ constructor: { [Symbol.species]: function () { return splitter; } }, flags: '' }, 'ab', { valueOf() { log += '3' + dep(); return 5; } }).join('/');
  const later = new RegExp(called.source + '|' + literal.source, 'v');
  const again = /a(b)?c/gi;
  return log + ':' + [literal.source, literal.flags, literal.lastIndex, made.source, made.flags, called.source, called.flags,
    constructed.source, constructed.flags, Object.getPrototypeOf(constructed) === RegExp.prototype, flags, text, String(later),
    again !== literal && again.lastIndex === 0 && again.flags === 'gi', found === null && made.lastIndex === 0, digit.groups.d + digit.index + counted.lastIndex, tested, answers,
    dashes, swapped, replaced, spaced, delegated, converted, matched, searched, step.value.index + String(step.done) + position, pieces].join();
}
