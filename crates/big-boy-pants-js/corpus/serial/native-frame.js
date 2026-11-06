// Native frames (light-cljs-786): `+` on an object runs its valueOf, or
// toString, in a resumable ToPrimitive frame, and an exception the method
// throws unwinds through that frame to a catch or finally block. A
// non-callable method is skipped, the left operand converts before the right,
// and an object neither method converts is a TypeError.
function nativeFrame() {
  const o = { valueOf: function () { return 41; } };
  const s = { valueOf: function () { return {}; }, toString: function () { return "s"; } };
  const t = { valueOf: function () { throw "boom"; } };
  const both = { valueOf: function () { return this === both ? 1 : 0; } };
  const skipped = { valueOf: 0, toString: function () { return "k"; } };
  let order = "";
  const left = { valueOf: function () { order += "l"; return "L"; } };
  const right = { valueOf: function () { order += "r"; return "R"; } };
  const neither = { valueOf: function () { return {}; }, toString: function () { return {}; } };
  let caught = "";
  try { caught = t + 1; } catch (e) { caught = e; }
  let ran = "";
  function f() { try { return t + 1; } finally { ran = "ran"; } }
  try { f(); } catch (e) { ran = ran + e; }
  let failed = "";
  try { failed = neither + 1; } catch (e) { failed = "TypeError"; } finally { failed += "!"; }
  return o + 1 === 42 && 1 + o === 42 && s + "!" === "s!" && both + both === 2 && caught === "boom" && ran === "ranboom" &&
    skipped + "" === "k" && left + right === "LR" && order === "lr" && failed === "TypeError!";
}
