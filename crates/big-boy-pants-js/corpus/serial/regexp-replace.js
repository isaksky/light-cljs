function regexpReplace() {
  // String.prototype.replace and replaceAll and RegExp.prototype[@@replace]
  // (light-cljs-934) at every budget cut and across collections inside the
  // job (serial_tests.goose): a global @@replace whose replacer runs for
  // every match, and whose empty matches advance lastIndex through a getter
  // and a valueOf; a generic object's @@replace with a user exec, a
  // lastIndex setter and results whose reads and captures run getters and
  // toString; `$<name>` reads through a groups getter; replaceAll with a
  // replacer over a string pattern and with a template; and many
  // replacements made and dropped, their pieces views of the input's run.
  let log = '';
  const input = 'ab1 cd ef2 ég ☃hi';
  const words = /(?<w>[a-z]*)(\d)?/gu;
  let out = '';
  for (let pass = 0; pass < 2; pass++) {
    out += input.replace(words, (m, w, d, p, s, g) => { log += 'f'; return m === '' ? '.' : '<' + g.w + (d === undefined ? '-' : d) + p + '>'; }) + '|';
  }
  // An exec that delegates to the built-in one and then leaves lastIndex
  // an object, so an empty match's advance converts it.
  const lazy = /b*/g;
  lazy.exec = function (s) {
    const m = RegExp.prototype.exec.call(this, s);
    const at = this.lastIndex;
    this.lastIndex = { valueOf() { log += 'v'; return at; } };
    return m;
  };
  const lazyOut = 'abbc'.replace(lazy, (m, p) => { log += 'r'; return '[' + m + p + ']'; });
  // A generic receiver: lastIndex through accessors, results of getters
  // and toString. (Its flags are data: V8 reads `global` and `unicode`.)
  let at = 0, n = 0;
  const generic = {
    flags: 'gu', global: true, unicode: true,
    get lastIndex() { log += 'L'; return { valueOf() { log += 'V'; return at; } }; },
    set lastIndex(v) { log += 'S'; at = v; },
    exec(s) {
      log += 'e';
      if (n++ >= 3) return null;
      const p = at;
      if (p === 0) at = 1;
      return { get 0() { log += '0'; return p === 0 ? 'a' : ''; }, get length() { log += 'l'; return 2; }, get index() { log += 'i'; return p; }, 1: { toString() { log += 't'; return 'c' + p; } }, groups: undefined };
    }
  };
  const genericOut = RegExp.prototype[Symbol.replace].call(generic, 'a\u{1F431}b', '($1:$&)');
  // Named groups through a getter, and a template's literal forms.
  const named = { flags: '', exec() { return { 0: 'cd', index: 3, length: 1, groups: { get x() { log += 'g'; return 'X'; } } }; } };
  const namedOut = RegExp.prototype[Symbol.replace].call(named, 'ab1cd', '[$<x>$<y>$$$`$\'$<x]');
  // replaceAll with string patterns.
  const dashes = 'a-b-c-d'.replaceAll('-', (m, p) => { log += 'd'; return { toString() { return '' + p; } }; });
  const munged = 'cljs.core.set-meta!?'.replaceAll('.', '$').replaceAll('-', '_').replaceAll('?', '_QMARK_');
  const kept = [];
  for (let i = 0; i < 60; i++) {
    const r = new RegExp('(\\d+)-(' + (i % 3 === 0 ? 'x' : 'y') + ')?', i % 2 === 0 ? 'g' : '');
    const s = 'k' + i + '-y' + i + '-x';
    const replaced = s.replace(r, '<$2$1>');
    const junk = [s.replaceAll('-', '+'), s.replace(/./gu, c => c + c)];
    if (i % 20 === 0) kept.push(replaced);
  }
  const each = '<ab10>. <cd-4>. <ef27>. .é<g-12>. .☃<hi-15>.|';
  return out === each + each && lazyOut === '[0]a[bb1][3]c[4]' && genericOut === '(c0:a)(c1:)\u{1F431}(c3:)b' && namedOut === 'ab1[X$ab1$<x]'
    && dashes === 'a1b3c5d' && munged === 'cljs$core$set_meta!_QMARK_' && kept.join() === 'k<0>y<x0>,k<y20><20>x,k<y40><40>x'
    && log === 'ffffffffffffffffffffffffvvvvrrrrSe0e0LVSe0LVSel0itl0itl0itgddd';
}
