// Synthetic fixture, not light-cljs output.
// Arithmetic, precedence, unary operators and const/let locals. The script
// completes with the value of its last expression statement: 27.5.
let a = 6, b = 4;
const c = a * b - a / b;   // 24 - 1.5 = 22.5
let d = (a + b) % 3;       // 1
let e = -(c - 2.5) / 4;    // -5
a = a - +b;                // 2
c + d * 10 + e + a * 0     // 22.5 + 10 - 5 + 0
