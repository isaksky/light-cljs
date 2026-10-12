// A compilation owns this registry. Canonical emitted functions and collections
// retain their normal runtime brands; only global storage differs by phase.
(() => {
  const units = new Map(), slots = new Map(), overlays = new Map(), path = [], extensions = new Map(), installing = [], bundles = [], frontiers = [];
  let frontier = Infinity, declarationFrontier = Infinity;
  // Initializers retain the declaration products visible at their source site.
  // This is a dynamic execution view, not a closure capture: an escaped factory
  // or record assoc method still reads the current constructor Var when called.
  function declarationSlot(name) {
    let slot = slots.get(name);
    while (slot?.bundle && !slot.bundle.group && slot.bundle.order > declarationFrontier) slot = slot.bundle.previous.get(name);
    return slot;
  }
  function force(name) {
    if (overlays.has(name)) return overlays.get(name);
    const owner = installing.at(-1);
    if (owner?.group && owner.names.includes(name)) return forceSlot(name, {unit: -1, bundle: owner});
    return forceSlot(name, declarationSlot(name));
  }
  function forceSlot(name, slot) {
    if (!slot || slot.unit === null) throw new Error((slot?.location ? slot.location + ': ' : '') + 'unavailable definition: ' + name);
    if (slot.bundle) {
      const bundle = slot.bundle;
      // Earlier products in the same executing group are already installed.
      // A forward read still forces the owner and reports a genuine cycle.
      if (bundle.group && bundle.state === 'initializing') {
        if (bundle.products.has(name)) return bundle.products.get(name);
        const previous = bundle.previous.get(name);
        if (previous && previous.unit !== null) return forceSlot(name, previous);
      }
      forceBundle(bundle);
      if (bundle.products.has(name)) return bundle.products.get(name);
      if (bundle.group) return forceSlot(name, bundle.previous.get(name) ?? {unit: null, location: bundle.location});
      return undefined;
    }
    if (slot.oncePrevious) {
      if (definitionAvailable(name, slot.oncePrevious)) return forceSlot(name, slot.oncePrevious);
      slot.oncePrevious = null;
    }
    if (slot.state === 'ready') return slot.value;
    if (slot.state === 'failed') throw slot.error;
    if (slot.state === 'initializing') {
      // Distinguish compilation reentry from an already analyzed root cycle.
      if (slot.code === undefined) globalThis.__lightCompileUnit(slot.unit);
      throw new Error('root initialization cycle: ' + [...path, name].join(' -> '));
    }
    const previousDeclarationFrontier = declarationFrontier;
    declarationFrontier = slot.declarationFrontier;
    slot.state = 'initializing'; path.push(name);
    try {
      const source = globalThis.__lightCompileUnit(slot.unit);
      slot.code = (0, eval)(source);
      slot.value = slot.code(__lightRuntimeForCompiler);
      slot.state = 'ready'; return slot.value;
    } catch (error) {
      if (error instanceof Error) error.message = slot.location + ': ' + name + ': ' + error.message;
      slot.state = 'failed'; slot.error = error; throw error;
    } finally { declarationFrontier = previousDeclarationFrontier; path.pop(); }
  }
  // A conditional owner can finish without installing its advertised product.
  // Plain pending roots still count as definitions without forcing their value.
  function definitionAvailable(name, slot) {
    if (!slot || slot.unit === null) return false;
    if (!slot.bundle?.group) return true;
    const bundle = slot.bundle;
    forceBundle(bundle);
    return bundle.products.has(name) || definitionAvailable(name, bundle.previous.get(name));
  }
  function forceBundle(bundle) {
    if (bundle.state === 'ready') return;
    if (bundle.state === 'failed') throw bundle.error;
    if (bundle.state === 'initializing') throw new Error('bundle initialization cycle: ' + [...path, bundle.name].join(' -> '));
    bundle.state = 'initializing'; path.push(bundle.name); installing.push(bundle);
    try {
      for (const [protocol] of bundle.indices) if (slots.has(protocol)) force(protocol);
      const installed = __lightRuntimeForCompiler.declarationTransaction(() => bundle.code(__lightRuntimeForCompiler), true);
      bundle.patches = installed.patches;
      bundle.state = 'ready';
    } catch (error) {
      bundle.products.clear(); bundle.state = 'failed'; bundle.error = error;
      if (error instanceof Error) error.message = bundle.location + ': ' + bundle.name + ': ' + error.message;
      throw error;
    } finally { installing.pop(); path.pop(); }
  }
  globalThis.__lightCompilerBundleProducts = new Proxy(Object.create(null), {
    get(_, name) {
      const bundle = installing.at(-1);
      if (bundle?.names.includes(name)) return bundle.products.get(name);
      return force(name);
    },
    set(_, name, value) {
      const bundle = installing.at(-1);
      if (bundle?.names.includes(name)) bundle.products.set(name, value);
      else slots.set(name, {unit: -1, state: 'ready', value});
      return true;
    }
  });
  globalThis.__lightRegisterBundle = (names, code, indices, location, once = false, group = false) => {
    const bundle = {group, previous: new Map(names.map(name => [name, slots.get(name)])), order: bundles.length + 1, names, indices, name: names[0] ?? 'extension ' + location, code, location, state: 'pending', products: new Map()};
    bundles.push(bundle);
    // Even a skipped defonce occupies its source frontier position. Rust's
    // catalog counts every declaration, including repeated product names.
    if (once && names.length && names.every(name => slots.has(name) && slots.get(name).unit !== null)) return;
    for (const name of names) slots.set(name, {unit: -1, bundle});
    for (const [protocol, selector, identity] of indices) {
      if (!extensions.has(protocol)) extensions.set(protocol, []);
      // A new declaration for the same protocol/target is a fresh generation.
      // Failed older generations remain sticky until this exact replacement.
      const entries = extensions.get(protocol);
      for (const entry of entries) if (entry.identity === identity && entry.bundle.state === 'failed') entry.superseded = bundle.order;
      entries.push({bundle, selector, identity});
    }
    __lightRuntimeForCompiler.setDeclarationDemand((protocol, value, typeName) => {
      const native = value == null ? 'null' : Array.isArray(value) ? 'array' : typeof value;
      const entries = typeof protocol === 'string' && protocol.startsWith('multifn:') && protocol !== 'multifn:*'
        ? [...(extensions.get(protocol) ?? []), ...(extensions.get('multifn:*') ?? [])]
            .sort((left, right) => left.bundle.order - right.bundle.order)
        : extensions.get(protocol) ?? [];
      for (const {bundle, selector, superseded} of entries) {
        if (bundle.order > frontier || superseded && frontier >= superseded) continue;
        if (selector === null || selector === '_' || selector === native || selector === '@' + typeName) forceBundle(bundle);
      }
    });
  };
  function selectFrontier(next) {
    if (frontier === next) return;
    if (!bundles.some(bundle => bundle.indices.length && bundle.state === 'ready' && (bundle.order <= frontier) !== (bundle.order <= next))) { frontier = next; return; }
    for (let i = bundles.length - 1; i >= 0; i--) {
      const bundle = bundles[i];
      if (bundle.indices.length && bundle.state === 'ready' && bundle.order <= frontier) __lightRuntimeForCompiler.applyDeclarationPatches(bundle.patches, false);
    }
    frontier = next;
    for (const bundle of bundles) if (bundle.indices.length && bundle.state === 'ready' && bundle.order <= frontier) __lightRuntimeForCompiler.applyDeclarationPatches(bundle.patches, true);
  }
  globalThis.__lightEnterCompilerFrontier = next => {
    const changesView = bundles.some(bundle => bundle.indices.length && bundle.state === 'ready' && (bundle.order <= frontier) !== (bundle.order <= next));
    const saved = changesView ? bundles.flatMap(bundle => bundle.indices.length && bundle.state === 'ready' ? __lightRuntimeForCompiler.snapshotDeclarationPatches(bundle.patches) : []) : null;
    frontiers.push({previous: frontier, saved}); selectFrontier(next); return null;
  };
  globalThis.__lightLeaveCompilerFrontier = () => {
    const {previous, saved} = frontiers.pop(); selectFrontier(previous);
    if (saved) __lightRuntimeForCompiler.restoreDeclarationPatches(saved);
    return null;
  };
  globalThis.__lightCompilerRoots = new Proxy(Object.create(null), {
    get(_, name) { return force(name); },
    set(_, name, value) {
      if (overlays.has(name)) overlays.set(name, value);
      else {
        force(name);
        const owner = installing.at(-1);
        if (owner?.group && owner.names.includes(name)) owner.products.set(name, value);
        else slots.set(name, {unit: -1, state: 'ready', value});
      }
      return true;
    }
  });
  globalThis.__lightHasCompilerRoot = name => overlays.has(name) || slots.has(name);
  // typeof must not read the demand proxy for an unavailable definition. An
  // existing root still needs its value: undefined is absent, nil/false exist.
  function rootExists(name, slot) {
    if (!slot || slot.unit === null) return false;
    if (slot.bundle?.group) {
      const bundle = slot.bundle;
      if (bundle.state !== 'initializing') forceBundle(bundle);
      if (bundle.products.has(name)) return bundle.products.get(name) !== undefined;
      const previous = bundle.previous.get(name);
      if (previous) return rootExists(name, previous);
      if (bundle.state !== 'initializing') return false;
      // A forward read inside its own executing group is a genuine cycle.
    }
    return forceSlot(name, slot) !== undefined;
  }
  globalThis.__lightCompilerRootExists = name => {
    if (overlays.has(name)) return overlays.get(name) !== undefined;
    const owner = installing.at(-1);
    return rootExists(name, owner?.group && owner.names.includes(name)
      ? {unit: -1, bundle: owner} : declarationSlot(name));
  };
  globalThis.__lightRegisterUnit = (name, unit, once, location, test) => {
    const old = slots.get(name);
    if (unit === null && old || once && old && old.unit !== null && !old.bundle?.group) return;
    const slot = {unit, test, location, declarationFrontier: bundles.length, state: 'pending', oncePrevious: once ? old : null};
    slots.set(name, slot);
    if (unit !== null) units.set(unit, slot);
  };
  globalThis.__lightCompilerTest = unit => forceSlot('executable :test metadata', units.get(unit));
  // Capture the source generation without demanding its root. The canonical
  // Var materializes this metadata only when reflection actually requests it.
  globalThis.__lightCompilerVarMetadata = name => {
    const slot = declarationSlot(name);
    if (overlays.has(name) || slot?.state === 'ready' || slot?.bundle?.products.has(name)) {
      const root = overlays.has(name) ? overlays.get(name)
        : slot.state === 'ready' ? slot.value : slot.bundle.products.get(name);
      const test = root != null && root !== false ? root.cljs$lang$test : null;
      return () => test;
    }
    return () => slot?.test == null ? null : globalThis.__lightCompilerTest(slot.test);
  };
  // Scope snapshots contain only overlays. Saving a pending/failed root must
  // neither demand nor replace it. Evaluate every initializer before installing.
  globalThis.__lightSaveCompilerBinding = name => [overlays.has(name), overlays.get(name)];
  globalThis.__lightBindCompilerRoot = (name, value) => { overlays.set(name, value); };
  globalThis.__lightRestoreCompilerBinding = (name, saved) => {
    if (saved[0]) overlays.set(name, saved[1]); else overlays.delete(name);
  };
  // Nested ordinary defs are executed expressions, so their initializer is eager.
  // Defonce nevertheless preserves any previously encountered source generation.
  globalThis.__lightDefineCompilerRoot = (name, initialize, once) => {
    const bundle = installing.at(-1);
    if (bundle?.names.includes(name)) {
      if (once && bundle.products.has(name)) return null;
      if (once && bundle.group) {
        const previous = bundle.previous.get(name);
        if (definitionAvailable(name, previous)) {
          return null;
        }
      }
      if (once && overlays.has(name) && overlays.get(name) !== undefined) return null;
      if (initialize === null) return undefined;
      const value = initialize();
      if (overlays.has(name)) overlays.set(name, value);
      else bundle.products.set(name, value);
      return value;
    }
    const old = slots.get(name);
    if (once && definitionAvailable(name, old)) return null;
    if (once && overlays.has(name) && overlays.get(name) !== undefined) return null;
    if (initialize === null) {
      if (!old) slots.set(name, {unit: null, state: 'pending'});
      return undefined;
    }
    const value = initialize();
    if (overlays.has(name)) overlays.set(name, value);
    else slots.set(name, {unit: -1, state: 'ready', value});
    return value;
  };
  return null;
})()
