function regexpSymbols() {
  // String.prototype.match, matchAll, search and split with RegExps, and
  // RegExp.prototype's @@match, @@matchAll, @@search and @@split
  // (light-cljs-937), at every budget cut and across collections inside the
  // job (serial_tests.goose): a global @@match whose exec delegates to the
  // built-in one, converts its results through toString and leaves
  // lastIndex an object, so empty matches advance it through a valueOf; a
  // generic receiver's @@search, its lastIndex saved and restored through
  // accessors; a matchAll iteration over a matcher a species getter's
  // constructor makes, whose exec runs user code and whose empty matches
  // advance through a valueOf; a split whose species getter, splitter
  // construction, lastIndex reads (a getter and a valueOf) and limit
  // conversion run user code; and many matches, iterators and pieces made
  // and dropped.
  let log = '';
  const input = 'ab1 cd22 ☃e3';
  const digits = input.match(/\d+/g).join();
  const first = input.match(/(?<w>[a-z]+)(\d)/);
  const dot = 'a.b'.match('.').index + ':' + 'a.b'.search('\\.');
  // A global @@match over a delegating exec.
  const lazy = /b*/g;
  lazy.exec = function (s) {
    const m = RegExp.prototype.exec.call(this, s);
    const at = this.lastIndex;
    this.lastIndex = { valueOf() { log += 'v'; return at; } };
    return m && { 0: { toString() { log += 't'; return m[0]; } } };
  };
  const lazyOut = 'abbc'.match(lazy).join('|');
  // A generic receiver's @@search.
  let last = 4;
  const generic = {
    get lastIndex() { log += 'L'; return last; }, set lastIndex(v) { log += 'S'; last = v; },
    exec(s) { log += 'e'; last = 7; return { get index() { log += 'i'; return s.length; } }; }
  };
  const found = RegExp.prototype[Symbol.search].call(generic, input);
  // matchAll over a species-made matcher.
  const species = /x/g;
  species.lastIndex = 1;
  species.constructor = {
    get [Symbol.species]() {
      log += 'p';
      return function (r, f) {
        log += 'n' + f;
        const made = new RegExp('[a-z]*', f);
        made.exec = function (s) {
          log += 'x';
          const m = RegExp.prototype.exec.call(this, s);
          if (m && m[0] === '') { const at = this.lastIndex; this.lastIndex = { valueOf() { log += 'w'; return at; } }; }
          return m;
        };
        return made;
      };
    }
  };
  const iterated = [...'ab1 cd'.matchAll(species)].map(m => m.index + ':' + m[0]).join();
  // A split through a species-made splitter whose lastIndex reads run user code.
  function Splitter(r, f) {
    log += 'k' + f;
    const inner = new RegExp(r.source, f);
    let at = 0;
    return {
      get lastIndex() { log += 'g'; return { valueOf() { log += 'o'; return at; } }; },
      set lastIndex(v) { at = v; },
      exec(s) { inner.lastIndex = at; const m = inner.exec(s); at = inner.lastIndex; return m; }
    };
  }
  const sep = /(-)/;
  sep.constructor = { get [Symbol.species]() { log += 's'; return Splitter; } };
  const pieces = 'a-b--c'.split(sep).join('|');
  const limited = 'x1y2z'.split(/\d/, { valueOf() { log += 'l'; return 2; } }).join();
  const kept = [];
  for (let i = 0; i < 60; i++) {
    const s = 'k' + i + '-v' + (i * 7) + '-w';
    const r = new RegExp('(\\d+)', i % 2 === 0 ? 'g' : '');
    const all = [...s.matchAll(/\d+/g)].map(m => m[0]).join('+');
    const parts = s.split(/-/);
    const m = s.match(r);
    const junk = [s.search(/w/), s.split(/(?:)/u).length, String(m), s.matchAll(/v/g).next().value.index];
    if (i % 20 === 0) kept.push(all + '/' + parts.length + '/' + m.join('.') + '/' + junk[0]);
  }
  return digits === '1,22,3' && first.groups.w + first[2] + first.index === 'ab10' && dot === '0:1'
    && lazyOut === '|bb||' && found === 12 && last === 4 && iterated === '1:b,2:,3:,4:cd,6:'
    && pieces === 'a|-|b|-||-|c' && limited === 'x,y' && kept.join() === '0+0/3/0.0/6,20+140/3/20.140/9,40+280/3/40.280/9'
    && log === 'tvtvtvtvLSeLSipngxxwxwxxwxskygogogol';
}
