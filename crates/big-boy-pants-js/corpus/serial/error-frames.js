// The Error constructors' message and cause steps and Error.prototype.toString's
// reads as native frames (light-cljs-870; decisions 4 and 5): a message's
// toString, a cause getter, name and message getters, a name object's
// toString and an array whose `join` is Error.prototype.toString each run a
// loop above an ErrorConstruct or ErrorText frame, and a throwing step
// unwinds it to the caller's catch, so serial_tests.goose stops and resumes
// those frames at every instruction and work limit, and collects inside the
// job at every cut while they hold the error, the options object, the
// receiver and a converted name made at run time.
function errorFrames() {
  let order = "";
  const spin = (tag) => { let i = 0; while (i < 3) i++; order += tag; };
  const e = new TypeError({ toString() { spin("m"); return "text"; } }, { get cause() { spin("c"); return this.n; }, n: 9 });
  const c = "c";
  const o = { get name() { spin("n"); return { toString() { spin("N"); return c + "ustom"; } }; }, get message() { spin("g"); return "msg"; } };
  const s = Error.prototype.toString.call(o);
  let thrown = 0;
  try { Error("m", { get cause() { spin("t"); throw 43; } }); } catch (x) { thrown = x; }
  const a = [1];
  a.join = Error.prototype.toString;
  a.name = { toString() { spin("j"); return "A"; } };
  a.message = "b";
  const joined = String(a);
  const plain = String(new RangeError("plain"));
  return order === "mcnNgtj" && e.message === "text" && e.cause === 9 && e instanceof TypeError && s === "custom: msg" && thrown === 43 && joined === "A: b" && plain === "RangeError: plain";
}
