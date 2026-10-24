// Synthetic fixture, not light-cljs output.
// Function declarations (hoisted), nested calls, direct and mutual
// recursion, function expressions and a higher-order function. The script
// completes with 11154.
let r = sumOfSquares(3, 4);           // hoisted: called before its declaration

function square(x) { return x * x; }
function sumOfSquares(a, b) { return square(a) + square(b); }   // nested call
function fact(n) { return n <= 1 ? 1 : n * fact(n - 1); }         // recursion
function isEven(n) { return n === 0 ? true : isOdd(n - 1); }      // mutual
function isOdd(n) { return n === 0 ? false : isEven(n - 1); }     // recursion

const compose = function (f, g) {
  return function (x) { return f(g(x)); };
};

r = r + fact(5);                      // 25 + 120 = 145
r = r + (isEven(10) ? 1000 : 0);      // 1145
r = r + (isOdd(7) ? 10000 : 0);       // 11145
r + compose(square, function (x) { return x + 1; })(2)   // + (2 + 1)^2 = 11154
