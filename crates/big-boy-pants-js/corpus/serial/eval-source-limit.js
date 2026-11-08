// Run-time code past what a snapshot of the job's code admits
// (light-cljs-856): every eval appends a 512 KiB comment's source to the
// code, so the seventeenth would pass 8 MiB; it fails MemoryLimit by name
// before the code changes (serial_tests.goose, with the job's unit and
// work limits lifted).
function evalSourceLimit() {
  var s = "x";
  while (s.length < 524288) s += s;
  var source = "/*" + s + "*/ 0";
  for (var i = 0; i < 40; i++) (0, eval)(source);
  return i;
}
