// Read-only unmapped arguments, ordinary/rest calls, receiver forwarding and
// escaped argument ownership. Real compiler-emitted wrappers are tested too.
function argumentCases() {
  'use strict';
  function ordinary(a) { 'use strict'; return arguments.length * 100 + (a === undefined ? 0 : a); }
  function rest(a, ...xs) { return xs.length * 100 + (xs[0] === undefined ? 0 : xs[0]); }
  function keep(a) { 'use strict'; return () => arguments; }
  function receiver(a) { 'use strict'; return this; }
  function inherited() { return this; }
  const a = keep(7, 8);
  const b = keep(9);
  const obj = { n: 4 };
  return ordinary() === 0 && ordinary(3) === 103 && ordinary(3, 4, 5) === 303
    && rest() === 0 && rest(1) === 0 && rest(1, 2, 3) === 202
    && a()[0] === 7 && a()[1] === 8 && a().length === 2 && a() === a()
    && b()[0] === 9 && b()[1] === undefined && a() !== b()
    && receiver.call() === undefined && receiver.call(null) === null
    && receiver.call(obj) === obj && receiver.call(3) === 3
    && receiver.call === receiver.call && receiver.call === rest.call
    && inherited.call(null) === null && inherited.call() === undefined;
}
