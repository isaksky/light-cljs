// A compilation owns this registry. Canonical emitted functions and collections
// retain their normal runtime brands; only global storage differs by phase.
(() => {
  const slots = new Map(), overlays = new Map(), path = [];
  function force(name) {
    if (overlays.has(name)) return overlays.get(name);
    const slot = slots.get(name);
    if (!slot || slot.unit === null) throw new Error((slot?.location ? slot.location + ': ' : '') + 'unavailable definition: ' + name);
    if (slot.state === 'ready') return slot.value;
    if (slot.state === 'failed') throw slot.error;
    if (slot.state === 'initializing') {
      // Distinguish compilation reentry from an already analyzed root cycle.
      if (slot.code === undefined) globalThis.__lightCompileUnit(slot.unit);
      throw new Error('root initialization cycle: ' + [...path, name].join(' -> '));
    }
    slot.state = 'initializing'; path.push(name);
    try {
      const source = globalThis.__lightCompileUnit(slot.unit);
      slot.code = (0, eval)(source);
      slot.value = slot.code(__lightRuntimeForCompiler);
      slot.state = 'ready'; return slot.value;
    } catch (error) {
      if (error instanceof Error) error.message = slot.location + ': ' + name + ': ' + error.message;
      slot.state = 'failed'; slot.error = error; throw error;
    } finally { path.pop(); }
  }
  globalThis.__lightCompilerRoots = new Proxy(Object.create(null), {
    get(_, name) { return force(name); },
    set(_, name, value) {
      if (overlays.has(name)) overlays.set(name, value);
      else { force(name); slots.get(name).value = value; }
      return true;
    }
  });
  globalThis.__lightHasCompilerRoot = name => overlays.has(name) || slots.has(name);
  globalThis.__lightRegisterUnit = (name, unit, once, location) => {
    const old = slots.get(name);
    if (unit === null && old || once && old && old.unit !== null) return;
    slots.set(name, {unit, location, state: 'pending'});
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
    const old = slots.get(name);
    if (once && old && old.unit !== null) return null;
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
