// A proxy's [[Get]], which light-cljs-766 implements, fails by name on
// Goose (light-cljs-845, decision 1): UnsupportedOperand naming
// `Proxy: [[Get]]`, which the `catch` cannot swallow (serial_tests.goose).
function proxyGet() {
  try { return new Proxy({ x: 1 }, {}).x; } catch (e) { return "caught"; }
}
