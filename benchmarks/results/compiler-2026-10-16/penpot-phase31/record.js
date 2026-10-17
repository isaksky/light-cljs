const evaluatedSources = [], functionSources = new WeakMap();
globalThis.__capturedUnits = Object.create(null);
function cleanCaptureDescriptor(d) {
  d[1] = null;
  if (d[2] != null) cleanCaptureDescriptor(d[2]);
  switch (d[0]) {
    case "list": case "vector": case "map": case "set":
      d[3].forEach(cleanCaptureDescriptor); break;
    case "regex": case "instant": case "uuid": case "queue": case "js-array": case "js-object":
      cleanCaptureDescriptor(d[3]); break;
    case "number":
      if (!Number.isFinite(d[3]) || Object.is(d[3], -0)) throw Error("capture cannot encode special Number");
      break;
  }
  return d;
}
function captureValue(value) {
  return cleanCaptureDescriptor(bridge.encode(value, 4000000000, 128));
}
function invokeRecorded(id, ids) {
  const fn = get(id), args = ids.map(get);
  if (!globalThis.__captureNextMacro) return Reflect.apply(fn, null, args);
  globalThis.__captureNextMacro = false;
  const capture = {sources: evaluatedSources.slice(), functionSource: functionSources.get(fn),
    arguments: args.map(captureValue), frontiers: globalThis.__captureFrontiers()};
  if (!capture.functionSource) throw Error("missing captured function source");
  const start = Date.now();
  const result = Reflect.apply(fn, null, args);
  capture.milliseconds = Date.now() - start;
  capture.expected = captureValue(result);
  capture.units = globalThis.__capturedUnits;
  globalThis.__macroCapture = capture;
  return result;
}
