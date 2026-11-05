function f() {
  let serial = 0;
  for (const kind of ['ordinary', 'pending', 'readyGetter', 'readyProxy']) {
    for (const fixed of ['different', 'same', 'noGetter']) {
      const target = {}, overlays = new Map(), readers = new Map();
      let traps = 0, getters = 0, mutations = 0;
      const proxy = new Proxy(target, {get(t, k, r) {
        traps++;
        return Reflect.get(t, k, r);
      }});
      const context = __rt.vector(proxy, target, overlays, readers);
      const cell = 200 + serial++;
      const mutate = () => {
        mutations++;
        __rt.need(cell, 1);
        Object.defineProperty(target, 'x', fixed === 'noGetter'
          ? {get: undefined, configurable: false}
          : {value: 2, writable: false, configurable: false});
        return fixed === 'same' ? 2 : 1;
      };
      const bundle = {state: kind === 'pending' ? 0 : 2, products: {x: 1}};
      if (kind === 'readyGetter') {
        Object.defineProperty(bundle.products, 'x', {get() {
          if (this !== bundle.products) throw new Error('product receiver');
          return mutate();
        }});
      }
      if (kind === 'readyProxy') {
        bundle.products = new Proxy({}, {get(t, k, r) {
          if (k !== 'x' || r !== bundle.products) throw new Error('product Proxy receiver');
          return mutate();
        }});
      }
      const getter = function() {
        getters++;
        if (this !== proxy) throw new Error('root receiver');
        if (kind === 'ordinary') return mutate();
        if (kind === 'pending') {
          bundle.products.x = mutate();
          bundle.state = 2;
        }
        return bundle.products.x;
      };
      Object.defineProperty(target, 'x', {configurable: true, get: getter});
      if (kind !== 'ordinary') readers.set(getter, __rt.vector(bundle, 'x'));
      let failed = false, value;
      try { value = __rt.cljReadRoot(proxy, context, 'x'); }
      catch (e) {
        if (!(e instanceof TypeError)) throw e;
        failed = true;
      }
      if (failed !== (fixed !== 'same') || (!failed && value !== 2))
        throw new Error('root invariant: ' + kind + '/' + fixed);
      if (traps !== 1 || getters !== 1 || mutations !== 1)
        throw new Error('root read replay: ' + kind + '/' + fixed);
    }
  }
  return true;
}
