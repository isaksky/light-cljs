// Script lexicals across suspensions (light-cljs-824): a generator and an
// async function declared in a script read and write its let and const
// bindings before and after a yield and an await. Completes with true
// once the promise jobs ran; a wrong value is an unhandled rejection.
let total = 1;
const base = 10;
function* walk() { total = total + base; yield total; total = total * 2; yield total + base; }
async function later() { total = total + 1; await null; total = total + base; if (total !== 33) { throw "order"; } }
var it = walk();
var first = it.next().value;
var second = it.next().value;
later();
first === 11 && second === 32 && total === 23;
