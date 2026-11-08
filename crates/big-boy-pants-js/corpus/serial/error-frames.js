// The Error constructors' message and cause steps and Error.prototype.toString's
// reads as native frames (light-cljs-870; decisions 4 and 5): a message's
// toString, a cause getter, name and message getters and a name object's
// toString each run a loop above an ErrorConstruct or ErrorText frame, and a
// throwing step unwinds it to the caller's catch, so serial_tests.goose stops
// and resumes those frames at every instruction and work limit.
function errorFrames() {
  let order = "";
  const spin = (tag) => { let i = 0; while (i < 3) i++; order += tag; };
  const e = new TypeError({ toString() { spin("m"); return "text"; } }, { get cause() { spin("c"); return this.n; }, n: 9 });
  const o = { get name() { spin("n"); return { toString() { spin("N"); return "custom"; } }; }, get message() { spin("g"); return "msg"; } };
  const s = Error.prototype.toString.call(o);
  let thrown = 0;
  try { Error("m", { get cause() { spin("t"); throw 43; } }); } catch (x) { thrown = x; }
  const plain = String(new RangeError("plain"));
  return order === "mcnNgt" && e.message === "text" && e.cause === 9 && e instanceof TypeError && s === "custom: msg" && thrown === 43 && plain === "RangeError: plain";
}
