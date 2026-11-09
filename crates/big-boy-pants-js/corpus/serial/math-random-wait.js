function mathRandomWait(dep) {
  // Math.random's stream across a dependency wait (light-cljs-901): `dep`
  // is a host effect the test answers with "D", in place or after the job's
  // whole stack was saved as a continuation and restored in another heap
  // (snapshot_tests.goose), whose header carries the stream's state, so the
  // draw after the wait continues the stream. A reused worker runs it twice:
  // its reset restores the image's state, so the second job draws the same
  // numbers, the Pants engine's first three from its fixed seed. Completes
  // with "D:true".
  const a = Math.random();
  const b = Math.random();
  const d = dep();
  const c = Math.random();
  return d + ":" + ([a, b, c].join() === "0.8833108082136426,0.43152799704850997,0.026433771592597743");
}
