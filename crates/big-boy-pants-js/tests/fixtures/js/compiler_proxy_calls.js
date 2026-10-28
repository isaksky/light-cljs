function f() {
  const log = [];
  let method = new Proxy(function(_, x, y) {
    if (this !== proxy) throw new Error('saved method receiver');
    log.push('method');
    return x + y;
  }, {apply(t, r, args) {
    if (r !== proxy) throw new Error('method apply receiver');
    log.push('apply');
    __rt.need(302, 1);
    return Reflect.apply(t, r, args);
  }});
  const proxy = new Proxy(function() { throw new Error('unexpected target call'); }, {
    get(t, k, r) {
      if (k !== 'call' || r !== proxy) throw new Error('call Proxy receiver');
      log.push('get');
      __rt.need(300, 1);
      return method;
    }
  });
  const saved = __rt.cljCallMember(proxy, null);
  log.push('arg');
  __rt.need(301, 1);
  method = () => 99;
  if (__rt.call(proxy, saved, null, 20, 22) !== 42) throw new Error('saved Proxy method');
  let replacement = Function.prototype.call;
  const callable = new Proxy(function(x) {
    'use strict';
    if (this !== null) throw new Error('target receiver');
    log.push('target');
    return x + 1;
  }, {
    get(t, k, r) {
      if (k !== 'call' || r !== callable) throw new Error('function Proxy receiver');
      log.push('getDefault');
      __rt.need(303, 1);
      return replacement;
    },
    apply(t, r, args) {
      if (r !== null) throw new Error('function apply receiver');
      log.push('applyDefault');
      __rt.need(305, 1);
      return Reflect.apply(t, r, args);
    }
  });
  const entry = __rt.cljCallMember(callable, null);
  log.push('argDefault');
  __rt.need(304, 1);
  replacement = () => 99;
  if (__rt.call(callable, entry, null, 41) !== 42) throw new Error('saved function method');
  if (log.join(',') !== 'get,arg,apply,method,getDefault,argDefault,applyDefault,target')
    throw new Error('Proxy calls replayed or reordered');
  return true;
}
