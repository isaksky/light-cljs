function captures() {
  function factory(base) { const offset = base + 3; return function(x) { return function(y) { return offset + x + y; }; }; }
  const first = factory(7)(2); const second = factory(20)(4);
  return first(1) === 13 && second(5) === 32 && first(3) === 15;
}
