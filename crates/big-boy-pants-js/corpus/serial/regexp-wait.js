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
  // a literal follow the last wait. 16 waits; completes with REGEXP_RESULT
  // there.
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
  const later = new RegExp(called.source + '|' + literal.source, 'v');
  const again = /a(b)?c/gi;
  return log + ':' + [literal.source, literal.flags, literal.lastIndex, made.source, made.flags, called.source, called.flags,
    constructed.source, constructed.flags, Object.getPrototypeOf(constructed) === RegExp.prototype, flags, text, String(later),
    again !== literal && again.lastIndex === 0 && again.flags === 'gi', found === null && made.lastIndex === 0, digit.groups.d + digit.index + counted.lastIndex, tested, answers].join();
}
