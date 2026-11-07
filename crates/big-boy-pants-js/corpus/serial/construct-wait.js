// A dependency wait inside a constructor (light-cljs-831): `dep` is a host
// effect the test answers with "D", in place or after the job's
// continuation was saved and restored in another heap
// (snapshot_tests.goose). The constructor's frame keeps its new object and
// its new.target across the wait, so the primitive it returns afterwards is
// replaced by the object. Completes with "aDb1".
function constructWait(dep) {
  function C(d) { this.log = "a"; this.v = d("need"); this.log += this.v + "b"; this.t = new.target === C; return 1; }
  const c = new C(dep);
  return c.log + (c.t && Object.getPrototypeOf(c) === C.prototype ? 1 : 0);
}
