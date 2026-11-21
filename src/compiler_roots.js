// A compilation owns this registry. Canonical emitted functions and collections
// retain their normal runtime brands; only global storage differs by phase.
(() => {
  // An extension environment data could reach that the runtime noted before
  // the compiler installed its report service (light-cljs-433).
  if (globalThis.__lightEnvironmentExtended) globalThis.__lightBuildService?.('environment-extension');
  const units = new Map(), slots = new Map(), overlays = new Map(), path = [], extensions = new Map(), installing = [], frontiers = [];
  let bundleCount = 0;
  // When Rust backs this engine with its catalog RootIndex (`__lightRootService`),
  // roots, :test units and bundles materialize lazily on first lookup instead of
  // being replayed eagerly on every registration or worker isolate.
  const rootService = typeof globalThis.__lightRootService === 'function' ? globalThis.__lightRootService : null;
  let catalogCursor = 0;
  const nameSynced = new Map(), extSynced = new Map(), bundleByOrder = new Map();
  const extendedProtocols = new Set();
  // Temporary per-namespace overlay window on a worker isolate (Stage C):
  // when active, lookups see registrations up to `overlayCursor` without
  // polluting persistent `catalogCursor` / `nameSynced` / `extSynced`.
  let overlayMode = false, overlayPrefix = 0, overlayPrefixBundles = 0, overlayCursor = 0, overlayBundles = 0;
  let overlaySavedFrontier = null;
  const overlayNameSynced = new Map(), overlayExtSynced = new Map(), overlayAddedProtocols = [];
  let demandInstalled = false;
  let frontier = Infinity, declarationFrontier = Infinity;
  // light.build records a read on every deferred unit whose initializer is
  // running: pairs of a unit and the compile depth it started at. When the
  // last one started at the current depth, its initializer is the innermost
  // code; otherwise a unit compiled since is, which Rust's catalog knows.
  const initializing = [];
  let compiling = 0;
  // While a --stats trace records (light-cljs-359), each force notes the
  // unit of the root or bundle it touches, ready ones included, once per
  // engine entry. The bridge check that ends the entry names the trace frame
  // it ran in, and only (frame, unit) pairs new since that unit's last read
  // go to Rust, in one call. An entry Rust makes inside __lightCompileUnit
  // may run in a unit's frame, so the caller's notes wait for its own check.
  let reads = null, readEpoch = 0, readService = null;
  const readMarks = [], readFrames = [];
  function noteRead(unit) {
    if (unit >= 0 && readMarks[unit] !== readEpoch) { readMarks[unit] = readEpoch; reads.push(unit); }
  }
  globalThis.__lightTraceRootReads = () => { readService = globalThis.__lightRootReadService; reads = []; return null; };
  globalThis.__lightRootReadsChecked = frame => {
    if (reads === null || reads.length === 0) return;
    let units = '';
    for (const unit of reads) {
      if (readFrames[unit] !== frame) { readFrames[unit] = frame; units += (units ? ',' : '') + unit; }
    }
    reads.length = 0; readEpoch++;
    if (units) readService(frame + ':' + units);
  };
  const compileService = globalThis.__lightCompileUnit;
  if (typeof compileService === 'function') {
    globalThis.__lightCompileUnit = unit => {
      compiling++;
      const outer = reads;
      if (outer !== null) { reads = []; readEpoch++; }
      try { return compileService(unit); } finally {
        compiling--;
        if (outer !== null) { for (const read of reads) outer.push(read); reads = outer; readEpoch++; }
      }
    };
  }
  function initialize(unit, run) {
    initializing.push(unit, compiling);
    try { return run(); } finally { initializing.length -= 2; }
  }
  globalThis.__lightBuildCaller = () => {
    let units = '';
    for (let i = 0; i < initializing.length; i += 2) units += (i ? ',' : '') + initializing[i];
    const innermost = initializing.length > 0 && initializing[initializing.length - 1] === compiling;
    return (innermost ? 'i' : 'e') + '\u0000' + units;
  };
  // Most bundles only produce roots. Frontier replay needs initialized
  // extensions, ordered by declaration rather than by initialization time.
  const readyExtensions = [];
  // First initialized extension after this declaration order.
  function readyAfter(order) {
    let low = 0, high = readyExtensions.length;
    while (low < high) {
      const middle = (low + high) >>> 1;
      if (readyExtensions[middle].order <= order) low = middle + 1;
      else high = middle;
    }
    return low;
  }
  function changesFrontier(next) {
    const low = Math.min(frontier, next), high = Math.max(frontier, next);
    const index = readyAfter(low);
    return index < readyExtensions.length && readyExtensions[index].order <= high;
  }
  function snapshotExtensions() {
    const saved = [], limit = overlayMode ? overlayBundles : bundleCount;
    for (let order = 0;;) {
      const bundle = readyExtensions[readyAfter(order)];
      if (!bundle || bundle.order > limit) break;
      order = bundle.order;
      for (const patch of __lightRuntimeForCompiler.snapshotDeclarationPatches(bundle.patches)) saved.push(patch);
    }
    return saved;
  }

  // Demanding a protocol only initializes matching pending bundles. Once a
  // selector has completed in this declaration view, repeating the scan cannot
  // do work. Never publish a result across registration or frontier reentry.
  const demanded = new Map();
  let demandEpoch = 0;
  function invalidateDemands() { demanded.clear(); demandEpoch++; }
  // The last demand found or recorded in `demanded`, which the operations
  // repeat for one protocol and type in a row: comparing it costs an
  // interpreter less than the lookups (light-cljs-439).
  let lastProtocol, lastNative, lastType, lastEpoch = -1;

  function makeBundleCode(unit) {
    let code;
    return rt => { code ??= (0, eval)(globalThis.__lightCompileUnit(unit)); return code(rt); };
  }

  function applyRoot(name, unit, once, location, test, declFrontier) {
    const old = slots.get(name);
    if (unit === null && old || once && old && old.unit !== null && !old.bundle?.group) return;
    const slot = {unit, test, location, declarationFrontier: declFrontier, state: 'pending', oncePrevious: once ? old : null};
    slots.set(name, slot);
    if (unit !== null) units.set(unit, slot);
  }

  function currentNameSynced(name) {
    if (overlayMode) {
      const ov = overlayNameSynced.get(name);
      if (ov !== undefined) return ov;
      return Math.min(nameSynced.get(name) ?? 0, overlayPrefix);
    }
    return nameSynced.get(name) ?? 0;
  }

  function advanceNameSynced(name, pos) {
    if (overlayMode) {
      if (pos > (overlayNameSynced.get(name) ?? 0)) overlayNameSynced.set(name, pos);
      const clamped = Math.min(pos, overlayPrefix);
      if (clamped > (nameSynced.get(name) ?? 0)) nameSynced.set(name, clamped);
    } else if (pos > (nameSynced.get(name) ?? 0)) {
      nameSynced.set(name, pos);
    }
  }

  function materializeBundle(pos, order, unit, names, indices, location, once, group) {
    const isOverlayLocal = overlayMode && pos >= overlayPrefix;
    if (!isOverlayLocal) {
      const existing = bundleByOrder.get(order);
      if (existing !== undefined) return existing;
    }
    // Bring every product to its exact state strictly before `pos` so
    // `previous` links and `once` checks match eager registration order.
    for (const n of names) ensureName(n, pos);
    const bundle = {
      unit,
      group,
      overlay: isOverlayLocal,
      previous: new Map(names.map(n => [n, slots.get(n)])),
      order,
      names,
      indices,
      name: names[0] ?? 'extension ' + location,
      code: makeBundleCode(unit),
      location,
      state: 'pending',
      products: new Map(),
      skipped: false,
      extRegistered: null
    };
    if (!isOverlayLocal) bundleByOrder.set(order, bundle);
    if (once && names.length && names.every(n => slots.has(n) && slots.get(n).unit !== null)) {
      bundle.skipped = true;
    } else {
      for (const n of names) slots.set(n, {unit: -1, bundle});
    }
    for (const n of names) advanceNameSynced(n, pos + 1);
    return bundle;
  }

  function ensureName(name, limit = -1) {
    if (rootService === null) return;
    const curCursor = overlayMode ? overlayCursor : catalogCursor;
    const prev = currentNameSynced(name);
    if (limit >= 0) {
      if (prev >= limit) return;
    } else if (slots.has(name) && prev >= curCursor) {
      return;
    }
    const raw = rootService('n\u0000' + name + '\u0000' + prev + '\u0000' + limit);
    if (!raw) {
      if (limit >= 0) advanceNameSynced(name, limit);
      else advanceNameSynced(name, curCursor);
      return;
    }
    const payload = JSON.parse(raw);
    const toPos = payload[0], entries = payload[1];
    for (const entry of entries) {
      const pos = entry[1];
      if (currentNameSynced(name) > pos) continue;
      if (entry[0] === 0) {
        advanceNameSynced(name, pos + 1);
        applyRoot(name, entry[2], entry[4], entry[5], entry[3], entry[6]);
      } else {
        advanceNameSynced(name, pos);
        materializeBundle(pos, entry[2], entry[3], entry[4], entry[5], entry[6], entry[7], entry[8]);
      }
    }
    advanceNameSynced(name, toPos);
  }

  function ensureProtocol(protocol) {
    if (rootService === null) return;
    const curCursor = overlayMode ? overlayCursor : catalogCursor;
    const prev = overlayMode
      ? (overlayExtSynced.get(protocol) ?? Math.min(extSynced.get(protocol) ?? 0, overlayPrefix))
      : (extSynced.get(protocol) ?? 0);
    if (prev >= curCursor) return;
    const raw = rootService('p\u0000' + protocol + '\u0000' + prev + '\u0000' + curCursor);
    if (overlayMode) {
      overlayExtSynced.set(protocol, curCursor);
      if (overlayPrefix > (extSynced.get(protocol) ?? 0)) extSynced.set(protocol, overlayPrefix);
    } else {
      extSynced.set(protocol, curCursor);
    }
    if (!raw) return;
    const list = JSON.parse(raw);
    for (const entry of list) {
      const bundle = materializeBundle(entry[0], entry[1], entry[2], entry[3], entry[4], entry[5], entry[6], entry[7]);
      if (bundle.skipped) continue;
      bundle.extRegistered ??= new Set();
      if (bundle.extRegistered.has(protocol)) continue;
      bundle.extRegistered.add(protocol);
      for (const [p, selector, identity] of bundle.indices) {
        if (p !== protocol) continue;
        if (!extensions.has(p)) extensions.set(p, []);
        const entries = extensions.get(p);
        for (const existing of entries) {
          if (existing.identity === identity && existing.bundle.state === 'failed') {
            existing.superseded = bundle.order;
          }
        }
        entries.push({bundle, selector, identity});
      }
    }
  }

  function declarationDemand(protocol, value, typeName) {
    if (frontier === 0) return;
    const native = value == null ? 'null' : Array.isArray(value) ? 'array' : typeof value;
    if (lastEpoch === demandEpoch && protocol === lastProtocol && typeName === lastType && native === lastNative) return;
    if (demanded.get(protocol)?.get(native)?.has(typeName)) { lastProtocol = protocol; lastNative = native; lastType = typeName; lastEpoch = demandEpoch; return; }
    if (rootService !== null) {
      ensureProtocol(protocol);
      if (typeof protocol === 'string' && protocol.startsWith('multifn:') && protocol !== 'multifn:*') {
        ensureProtocol('multifn:*');
      }
    }
    const epoch = demandEpoch;
    const entries = typeof protocol === 'string' && protocol.startsWith('multifn:') && protocol !== 'multifn:*'
      ? [...(extensions.get(protocol) ?? []), ...(extensions.get('multifn:*') ?? [])]
          .sort((left, right) => left.bundle.order - right.bundle.order)
      : extensions.get(protocol) ?? [];
    for (const {bundle, selector, superseded} of entries) {
      if (overlayMode && !bundle.overlay && bundle.order > overlayPrefixBundles) continue;
      const effSuperseded = superseded && !(overlayMode && superseded > overlayPrefixBundles) ? superseded : 0;
      if (bundle.order > frontier || effSuperseded && frontier >= effSuperseded) continue;
      if (selector === null || selector === '_' || selector === native || selector === '@' + typeName) forceBundle(bundle);
    }
    if (epoch === demandEpoch) {
      let selectors = demanded.get(protocol);
      if (!selectors) demanded.set(protocol, selectors = new Map());
      let types = selectors.get(native);
      if (!types) selectors.set(native, types = new Set());
      types.add(typeName);
      lastProtocol = protocol; lastNative = native; lastType = typeName; lastEpoch = demandEpoch;
    }
  }

  function ensureDemandInstalled() {
    if (demandInstalled) return;
    demandInstalled = true;
    // A protocol no bundle extends has no entries to force, so the
    // compile-time runtime calls this only for the protocols in
    // `extendedProtocols` (build.rs, `interpreterDispatch`).
    __lightRuntimeForCompiler.setDeclarationDemand(declarationDemand, extendedProtocols);
  }

  globalThis.__lightSyncCatalog = (nextCursor, nextBundles, newProtocols) => {
    if (newProtocols && newProtocols.length) {
      const tag = newProtocols[0];
      if (tag === '\u0000clear') {
        overlayMode = false;
        overlayNameSynced.clear();
        overlayExtSynced.clear();
        for (const p of overlayAddedProtocols) extendedProtocols.delete(p);
        overlayAddedProtocols.length = 0;
        if (overlaySavedFrontier !== null) {
          const {previous, saved} = overlaySavedFrontier;
          overlaySavedFrontier = null;
          selectFrontier(previous);
          if (saved) __lightRuntimeForCompiler.restoreDeclarationPatches(saved);
        }
        invalidateDemands();
        return;
      }
      if (tag === '\u0000overlay') {
        const nextPrefix = newProtocols.length > 1 ? Number(newProtocols[1]) : catalogCursor;
        const nextPrefixBundles = newProtocols.length > 2 ? Number(newProtocols[2]) : bundleCount;
        if (!overlayMode) {
          const changesView = changesFrontier(nextPrefixBundles);
          const saved = changesView ? snapshotExtensions() : null;
          overlaySavedFrontier = {previous: frontier, saved};
          selectFrontier(nextPrefixBundles);
        }
        overlayPrefix = nextPrefix;
        overlayPrefixBundles = nextPrefixBundles;
        overlayMode = true;
        overlayCursor = nextCursor;
        overlayBundles = nextBundles;
        if (nextBundles > frontier && frontiers.length === 0) {
          frontier = nextBundles;
        }
        let added = false;
        for (let i = 3; i < newProtocols.length; i++) {
          const p = newProtocols[i];
          if (!extendedProtocols.has(p)) {
            extendedProtocols.add(p);
            overlayAddedProtocols.push(p);
            added = true;
          }
        }
        if (added) ensureDemandInstalled();
        invalidateDemands();
        return;
      }
    }
    if (nextCursor > catalogCursor) catalogCursor = nextCursor;
    if (nextBundles > bundleCount) {
      bundleCount = nextBundles;
      invalidateDemands();
    }
    if (newProtocols && newProtocols.length) {
      for (const protocol of newProtocols) extendedProtocols.add(protocol);
      ensureDemandInstalled();
    }
  };

  // Initializers retain the declaration products visible at their source site.
  // This is a dynamic execution view, not a closure capture: an escaped factory
  // or record assoc method still reads the current constructor Var when called.
  function declarationSlot(name) {
    ensureName(name);
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
    if (reads !== null) noteRead(slot.unit);
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
      slot.value = initialize(slot.unit, () => slot.code(__lightRuntimeForCompiler));
      slot.state = 'ready'; return slot.value;
    } catch (error) {
      if (error instanceof Error) error.message = slot.location + ': ' + name + ': ' + error.message;
      slot.state = 'failed'; slot.error = error; throw error;
    } finally {
      // Ready roots retain their value, failed roots their exception. Neither
      // runs its initializer again; keeping it also keeps its source/catalog.
      slot.code = undefined;
      declarationFrontier = previousDeclarationFrontier; path.pop();
    }
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
    if (reads !== null) noteRead(bundle.unit);
    if (bundle.state === 'ready') return;
    if (bundle.state === 'failed') throw bundle.error;
    if (bundle.state === 'initializing') throw new Error('bundle initialization cycle: ' + [...path, bundle.name].join(' -> '));
    bundle.state = 'initializing'; path.push(bundle.name); installing.push(bundle);
    try {
      for (const [protocol] of bundle.indices) {
        ensureName(protocol);
        if (slots.has(protocol)) force(protocol);
      }
      const installed = __lightRuntimeForCompiler.declarationTransaction(() => initialize(bundle.unit, () => bundle.code(__lightRuntimeForCompiler)), true);
      bundle.patches = installed.patches;
      bundle.state = 'ready';
      if (bundle.indices.length) readyExtensions.splice(readyAfter(bundle.order), 0, bundle);
    } catch (error) {
      bundle.products.clear(); bundle.state = 'failed'; bundle.error = error;
      if (error instanceof Error) error.message = bundle.location + ': ' + bundle.name + ': ' + error.message;
      throw error;
    } finally {
      bundle.code = undefined;
      installing.pop(); path.pop();
    }
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
      else {
        ensureName(name);
        slots.set(name, {unit: -1, state: 'ready', value});
        if (rootService !== null) advanceNameSynced(name, catalogCursor);
      }
      return true;
    }
  });
  globalThis.__lightRegisterBundle = (names, code, indices, location, once = false, group = false, unit = -1) => {
    invalidateDemands();
    for (const name of names) ensureName(name);
    const bundle = {unit, group, previous: new Map(names.map(name => [name, slots.get(name)])), order: ++bundleCount, names, indices, name: names[0] ?? 'extension ' + location, code, location, state: 'pending', products: new Map()};
    // Even a skipped defonce occupies its source frontier position. Rust's
    // catalog counts every declaration, including repeated product names.
    if (once && names.length && names.every(name => slots.has(name) && slots.get(name).unit !== null)) return;
    for (const name of names) {
      slots.set(name, {unit: -1, bundle});
      if (rootService !== null) advanceNameSynced(name, catalogCursor);
    }
    for (const [protocol, selector, identity] of indices) {
      if (rootService !== null) ensureProtocol(protocol);
      extendedProtocols.add(protocol);
      if (!extensions.has(protocol)) extensions.set(protocol, []);
      // A new declaration for the same protocol/target is a fresh generation.
      // Failed older generations remain sticky until this exact replacement.
      const entries = extensions.get(protocol);
      for (const entry of entries) if (entry.identity === identity && entry.bundle.state === 'failed') entry.superseded = bundle.order;
      entries.push({bundle, selector, identity});
    }
    ensureDemandInstalled();
  };
  function selectFrontier(next) {
    if (frontier === next) return;
    invalidateDemands();
    if (!changesFrontier(next)) { frontier = next; return; }
    // A Proxy's declaration patch may reenter and initialize another bundle.
    // Advance by source order so inserting a ready entry cannot revisit or
    // skip one. As before, reverse replay excludes newly registered bundles.
    for (let order = bundleCount + 1;;) {
      const bundle = readyExtensions[readyAfter(order - 1) - 1];
      if (!bundle) break;
      order = bundle.order;
      if (bundle.order <= frontier) __lightRuntimeForCompiler.applyDeclarationPatches(bundle.patches, false);
    }
    frontier = next;
    for (let order = 0;;) {
      const bundle = readyExtensions[readyAfter(order)];
      if (!bundle) break;
      order = bundle.order;
      if (bundle.order <= frontier && !(overlayMode && !bundle.overlay && bundle.order > overlayPrefixBundles)) {
        __lightRuntimeForCompiler.applyDeclarationPatches(bundle.patches, true);
      }
    }
  }
  globalThis.__lightEnterCompilerFrontier = next => {
    const changesView = changesFrontier(next);
    const saved = changesView ? snapshotExtensions() : null;
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
        else {
          slots.set(name, {unit: -1, state: 'ready', value});
          if (rootService !== null) advanceNameSynced(name, catalogCursor);
        }
      }
      return true;
    }
  });
  globalThis.__lightHasCompilerRoot = name => {
    if (overlays.has(name)) return true;
    ensureName(name);
    return slots.has(name);
  };
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
    ensureName(name);
    applyRoot(name, unit, once, location, test, bundleCount);
    if (rootService !== null) advanceNameSynced(name, catalogCursor);
  };
  globalThis.__lightCompilerTest = unit => {
    if (!units.has(unit) && rootService !== null) {
      const raw = rootService('t\u0000' + unit);
      if (raw) {
        const [pos, name] = JSON.parse(raw);
        ensureName(name, pos + 1);
      }
    }
    return forceSlot('executable :test metadata', units.get(unit));
  };
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
  // Whether a defonce of name keeps a value already defined, without
  // evaluating its initializer. An ^:async function's nested defonce asks
  // first, since its initializer may await and cannot be a thunk.
  const keptOnce = name => {
    const bundle = installing.at(-1);
    if (bundle?.names.includes(name)) {
      if (bundle.products.has(name)) return true;
      if (bundle.group && definitionAvailable(name, bundle.previous.get(name))) return true;
    } else {
      ensureName(name);
      if (definitionAvailable(name, slots.get(name))) return true;
    }
    return overlays.has(name) && overlays.get(name) !== undefined;
  };
  globalThis.__lightCompilerRootKept = keptOnce;
  // Nested ordinary defs are executed expressions, so their initializer is eager.
  // Defonce nevertheless preserves any previously encountered source generation.
  globalThis.__lightDefineCompilerRoot = (name, initialize, once) => {
    if (once && keptOnce(name)) return null;
    const bundle = installing.at(-1);
    if (bundle?.names.includes(name)) {
      if (initialize === null) return undefined;
      const value = initialize();
      if (overlays.has(name)) overlays.set(name, value);
      else bundle.products.set(name, value);
      return value;
    }
    ensureName(name);
    const old = slots.get(name);
    if (initialize === null) {
      if (!old) {
        slots.set(name, {unit: null, state: 'pending'});
        if (rootService !== null) advanceNameSynced(name, catalogCursor);
      }
      return undefined;
    }
    const value = initialize();
    if (overlays.has(name)) overlays.set(name, value);
    else {
      slots.set(name, {unit: -1, state: 'ready', value});
      if (rootService !== null) advanceNameSynced(name, catalogCursor);
    }
    return value;
  };
  return null;
})()
