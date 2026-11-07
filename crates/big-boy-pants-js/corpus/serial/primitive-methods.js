function primitiveMethods(n) {
  // Property reads and method calls on number, string and boolean
  // primitives (light-cljs-833) make no wrapper: serial_tests.goose runs
  // this with n = 0 and n = 300 and compares the job's node rows, which
  // must not grow with n. Without an argument (Node and the Pants engine)
  // it runs 200 rounds. Completes with the round count plus every round's
  // checks, so 2 * n.
  if (n === undefined) { n = 200; }
  let r = 0;
  const s = "abc";
  for (let i = 0; i < n; i++) {
    r = r + i.valueOf() - i;
    if (i.toString() === "" + i && s.toString() === s && s.valueOf().length === 3 && s[1] === "b") { r = r + 1; }
    if (true.toString() === "true" && false.valueOf() === false && (1.5).toString() === "1.5" && (i).toString === Number.prototype.toString) { r = r + 1; }
  }
  return r;
}
