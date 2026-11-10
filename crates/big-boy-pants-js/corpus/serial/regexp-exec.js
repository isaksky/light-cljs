function regexpExec() {
  // RegExp.prototype.exec and test (light-cljs-933) at every budget cut
  // and across collections inside the job (serial_tests.goose): exec's
  // frame converting its argument through toString and lastIndex through
  // valueOf, test's frame reading `exec` through a getter and calling a
  // user exec, global and sticky lastIndex updates, named groups and
  // indices, Unicode mode over pairs, and many match results made and
  // dropped, their group strings views of the input's run.
  let log = '';
  const global = /(?<word>[a-z]+)(\d)?/g;
  const input = 'ab1 cd ef2 ég ☃hi';
  let words = '';
  for (let pass = 0; pass < 3; pass++) {
    global.lastIndex = { valueOf() { log += 'v'; return 0; } };
    let m;
    while ((m = global.exec({ toString() { log += 't'; return input; } })) !== null) {
      words += m.groups.word + (m[2] === undefined ? '-' : m[2]) + m.index + ',';
    }
  }
  const sticky = /a(?=b)/y;
  sticky.lastIndex = 2;
  const stuck = [sticky.test('xxab'), sticky.lastIndex, sticky.test('xxab'), sticky.lastIndex].join();
  const user = /z/;
  user.exec = function (s) { log += 'e'; return s === 'yes' ? { 0: s } : null; };
  const answers = [user.test('yes'), user.test('no')].join();
  const read = /q/;
  Object.defineProperty(read, 'exec', { get() { log += 'g'; return undefined; } });
  const fallback = [read.test('aqa'), read.test('b')].join();
  const generic = RegExp.prototype.test.call({ get exec() { log += 'x'; return () => ({}); } }, 'k');
  const d = /(?<n>\u{1F431})(x)?/dgu;
  d.lastIndex = 2;
  const pair = d.exec('a\u{1F431}\u{1F431}');
  const indices = [pair.index, pair.indices[0][0], pair.indices[0][1], pair.indices.groups.n === pair.indices[1], pair.indices[2], d.lastIndex].join();
  const kept = [];
  for (let i = 0; i < 60; i++) {
    const r = new RegExp('(\\d+)-(' + (i % 3 === 0 ? 'x' : 'y') + ')?', i % 2 === 0 ? 'g' : '');
    const m = r.exec('k' + i + '-y' + i);
    const junk = [m, /./u.exec('\u{10000}'), r.test('none')];
    if (i % 20 === 0) kept.push(m);
  }
  const keptText = kept.map(m => m[1] + ':' + m[2] + ':' + m.index).join();
  const each = 'ab10,cd-4,ef27,g-12,hi-15,';
  return log === 'tvttttttvttttttvttttteeggx' && words === each + each + each && stuck === 'true,3,false,0' && answers === 'true,false' && fallback === 'true,false'
    && generic === true && indices === '1,1,3,true,,3' && keptText === '0:undefined:1,20:y:1,40:y:1';
}
