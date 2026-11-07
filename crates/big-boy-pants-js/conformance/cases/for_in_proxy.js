// bbp-authored conformance case.
// Derives from: crates/big-boy-pants-js/rust/tests/js_source.rs lines 29274-29339
// (`proxy_own_keys_and_descriptor_traps`, which quotes QuickJS
// tests/test_loop.js test_for_in_proxy, and its TypeError rows without
// Object.defineProperty), lines 29340-29415
// (`proxy_invariants_reflection_and_forwarding`, its for-in and forwarding
// rows) and lines 29416-29440
// (`proxy_descriptor_observation_order_and_receivers`) (light-cljs-845;
// ECMAScript 10.5.5 [[GetOwnProperty]], 10.5.11 [[OwnPropertyKeys]],
// 14.7.5.9 EnumerateObjectProperties). for-in over a proxy lists its keys
// through [[OwnPropertyKeys]] once, asks [[GetOwnProperty]] for each key
// just before visiting it and walks on to the target's prototype. Rows that
// need Object.defineProperty (a non-configurable or non-enumerable target
// key other than an array's `length`), Object.preventExtensions or a `get`
// trap wait for light-cljs-766. QuickJS asks a proxy for each descriptor
// twice (when it lists the keys and again before each visit), accepts a
// non-object ownKeys result and lets a key reported missing hide a
// prototype's name, where the Pants engine, V8 and the specification do
// not, so this case counts no descriptor calls and leaves those rows to the
// Goose serial corpus (corpus/serial/proxy-for-in.js); the getPrototypeOf
// trap, which the Pants engine does not implement, is
// bbp/for_in_proxy_prototype. Completes with
// "abc| a c|gkhdaecv:ab|13|function,object||akd".
(function () {
  var out = [];
  // The first n code units of s (String.prototype.slice is pending).
  function prefix(s, n) { var p = ""; for (var j = 0; j < n && j < s.length; j++) p += s[j]; return p; }
  // No traps: the target's keys, then its prototype's.
  var s = "";
  for (var k in new Proxy({ __proto__: { c: 3 }, a: 1, b: 2 }, {})) s += k;
  out.push(s);
  // test_for_in_proxy: a key the descriptor trap reports missing once the
  // loop has started is skipped.
  var removed = "";
  var p = new Proxy({}, {
    ownKeys: function () { return ["a", "b", "c"]; },
    getOwnPropertyDescriptor: function (target, key) {
      if (removed != "" && key == removed) return undefined;
      return { enumerable: true, configurable: true, value: this[key] };
    }
  });
  s = "";
  for (var o in p) { s += " " + o; if (o == "a") removed = "b"; }
  out.push(s);
  // Traps that are getters on the handler, a length and an element getter,
  // and the first descriptor's fields read in ToPropertyDescriptor's order.
  var log = "";
  var handler = {
    get ownKeys() {
      log += "g";
      return function () { log += "k"; return { get length() { return 2; }, 0: "a", get 1() { return "b"; } }; };
    },
    get getOwnPropertyDescriptor() {
      log += "h";
      return function (t, k) {
        log += "d" + k;
        return { get enumerable() { log += "e"; return true; }, get configurable() { log += "c"; return true; }, get value() { log += "v"; return 1; } };
      };
    }
  };
  s = "";
  for (var k2 in new Proxy({}, handler)) s += k2;
  out.push(prefix(log, 8) + ":" + s);
  // TypeErrors: no `new`, a non-object target or handler, a duplicate or a
  // number key, an omitted non-configurable target key (an array's
  // `length`), a missing report of one, a descriptor with a non-callable
  // getter, with both a getter and a value, reported non-configurable for a
  // missing key, non-writable for a writable `length`, enumerable for a
  // non-enumerable `length`, and a descriptor result that is neither an
  // object nor undefined.
  var n = 0;
  var rows = [
    function () { Proxy({}, {}); },
    function () { new Proxy(1, {}); },
    function () { new Proxy({}, null); },
    function () { for (var x in new Proxy({}, { ownKeys: function () { return ["a", "a"]; } })) {} },
    function () { for (var x in new Proxy({}, { ownKeys: function () { return [1]; } })) {} },
    function () { for (var x in new Proxy([], { ownKeys: function () { return []; } })) {} },
    function () { for (var x in new Proxy([], { ownKeys: function () { return ["length"]; }, getOwnPropertyDescriptor: function () { return undefined; } })) {} },
    function () { for (var x in new Proxy({}, { ownKeys: function () { return ["a"]; }, getOwnPropertyDescriptor: function () { return { get: 1, configurable: true }; } })) {} },
    function () { for (var x in new Proxy({}, { ownKeys: function () { return ["a"]; }, getOwnPropertyDescriptor: function () { return { get: function () {}, value: 1, configurable: true }; } })) {} },
    function () { for (var x in new Proxy({}, { ownKeys: function () { return ["a"]; }, getOwnPropertyDescriptor: function () { return { value: 1, enumerable: true, configurable: false }; } })) {} },
    function () { for (var x in new Proxy([], { ownKeys: function () { return ["length"]; }, getOwnPropertyDescriptor: function () { return { value: 0, writable: false, enumerable: false, configurable: false }; } })) {} },
    function () { for (var x in new Proxy([], { ownKeys: function () { return ["length"]; }, getOwnPropertyDescriptor: function () { return { value: 0, writable: true, enumerable: true, configurable: false }; } })) {} },
    function () { for (var x in new Proxy({}, { getOwnPropertyDescriptor: function () { return 1; }, ownKeys: function () { return ["a"]; } })) {} },
  ];
  for (var i = 0; i < rows.length; i++) { try { rows[i](); } catch (e) { if (e instanceof TypeError) n++; } }
  out.push(String(n));
  // typeof follows the target's callability.
  out.push(typeof new Proxy(function () {}, {}) + "," + typeof new Proxy({}, {}));
  // A key reported non-enumerable hides its prototype's name.
  s = "";
  for (var k5 in new Proxy({ __proto__: { x: 1 } }, { ownKeys: function () { return ["x"]; }, getOwnPropertyDescriptor: function () { return { enumerable: false, configurable: true }; } })) s += k5;
  out.push(s);
  // A proxy without traps forwards to a proxy target's traps.
  var calls = "";
  var inner = new Proxy({ a: 1 }, {
    ownKeys: function () { calls += "k"; return ["a"]; },
    getOwnPropertyDescriptor: function (t, k) { calls += "d"; return { value: t[k], writable: true, enumerable: true, configurable: true }; }
  });
  s = "";
  for (var k6 in new Proxy(inner, {})) s += k6;
  out.push(s + prefix(calls, 2));
  return out.join("|");
})();
