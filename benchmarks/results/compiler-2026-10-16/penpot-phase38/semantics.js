(() => {
  const checks = [];
  function check(label, value) { if (!value) throw Error(label); checks.push(label); }
  const nativeCall = Function.prototype.call;
  function strict(a, b) { 'use strict'; const old = a; a = {changed: true}; return [this, old, a, b, arguments.length, arguments[0]]; }
  const original = {original: true}, extra = {extra: true};
  for (const receiver of [null, undefined, false, 42, 'text', original]) {
    const result = strict.call(receiver, original, extra);
    check('strict receiver/arguments', result[0] === receiver && result[1] === original && result[2].changed && result[3] === extra && result[4] === 2 && result[5] === original);
  }
  check('omitted arguments', strict.call(null)[4] === 0 && strict.call(null)[1] === undefined && strict.call()[0] === undefined);
  const sloppy = Function('a', 'b', 'a={changed:true}; return [this,a,b,arguments[0],arguments.length];');
  const mapped = sloppy.call(null, original, extra, original);
  check('sloppy mapped arguments', mapped[0] === globalThis && mapped[1] === mapped[3] && mapped[2] === extra && mapped[4] === 3);
  check('sloppy primitive receiver', sloppy.call(42)[0].valueOf() === 42);
  const capture = Function('a', 'b', 'arguments[0]={changed:true}; const args=arguments; return [()=>a,args,()=>b];');
  for (let i = 0; i < 2000; i++) {
    const result = capture.call(null, original, extra, original);
    check('captured mapped arguments', result[0]() === result[1][0] && result[0]().changed && result[2]() === extra && result[1][2] === original);
    result[1][0] = extra;
    check('captured argument mutation', result[0]() === extra);
  }
  const missing = capture.call(null);
  check('missing mapped parameter', missing[0]() === undefined && missing[1][0].changed && missing[2]() === undefined);
  let escaped;
  const failing = Function('a', 'save', 'a={changed:true}; save(()=>a); throw a;');
  try { failing.call(null, original, value => { escaped = value; }); } catch (error) {
    check('exception closes parameter', error === escaped() && error.changed);
  }
  function inner(a) { a = {nested: a}; return () => a; }
  const nested = nativeCall.call(nativeCall, inner, null, original);
  check('nested borrowed calls', nested().nested === original);
  const receiver = {};
  function boundTarget(a, b) { return [this, a, b]; }
  const bound = boundTarget.bind(receiver, original);
  const bounded = bound.call(null, extra);
  check('bound target', bounded[0] === receiver && bounded[1] === original && bounded[2] === extra);
  let reads = 0;
  function custom() { throw Error('wrong function'); }
  Object.defineProperty(custom, 'call', {get() { reads++; return function (a) { return [this, a]; }; }});
  const overridden = custom.call(extra);
  check('own call getter', reads === 1 && overridden[0] === custom && overridden[1] === extra);
  let applied;
  const proxy = new Proxy(inner, {apply(target, self, args) { applied = [self, args.slice()]; return Reflect.apply(target, self, args); }});
  check('proxy callable', proxy.call(receiver, original)().nested === original && applied[0] === receiver && applied[1][0] === original);
  const reflected = Reflect.apply(nativeCall, strict, [receiver, original, extra]);
  check('borrowed Reflect.apply inputs', reflected[0] === receiver && reflected[1] === original && reflected[3] === extra);
  class ClassOnly {}
  let rejected = 0;
  try { ClassOnly.call(null); } catch (error) { if (error instanceof TypeError) rejected++; }
  try { nativeCall.call({}, null); } catch (error) { if (error instanceof TypeError) rejected++; }
  check('noncallable/class errors', rejected === 2);
  let replacementReceiver;
  try {
    Function.prototype.call = function (value) { replacementReceiver = this; return value; };
    check('replaced Function.call', inner.call(extra) === extra && replacementReceiver === inner);
  } finally { Function.prototype.call = nativeCall; }
  function stackTarget(a) { a = extra; return new Error('stack').stack; }
  Error.prepareStackTrace = (_error, sites) => sites.slice(0, 3).map(site => [site.getFunctionName(), site.isNative()]);
  const stack = stackTarget.call(null, original);
  check('native call frame retained', stack[0][0] === 'stackTarget' && stack[1][0] === 'call' && stack[1][1] === true);
  delete Error.prepareStackTrace;
  return JSON.stringify({checks: checks.length, stack});
})()
