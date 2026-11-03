function f() {
  const original = globalThis;
  const self = globalThis.globalThis === globalThis && original.missingGlobalForSourceRegression === undefined;
  original.globalThis = {marker: 17};
  const changed = globalThis === original.globalThis && globalThis.marker === 17;
  original.globalThis = undefined;
  const cleared = globalThis === undefined && original.globalThis === undefined;
  original.globalThis = original;
  return self && changed && cleared && globalThis === original;
}
