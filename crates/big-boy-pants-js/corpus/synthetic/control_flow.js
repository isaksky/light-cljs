// Synthetic fixture, not light-cljs output.
// A while loop over 0..19 with an if / else-if / else chain, nested blocks
// and a block-scoped binding that shadows the loop counter. It sums the odd
// numbers that are not multiples of 3 (1 5 7 11 13 17 19 = 73) and counts
// them (7); the script completes with sum * 100 + count = 7307.
let i = 0;
let sum = 0;
let count = 0;
while (i < 20) {
  let odd = i % 2 === 1;
  if (odd && i % 3 !== 0) {
    sum = sum + i;
    count = count + 1;
  } else if (!odd) {
    let i = 100;           // shadows the loop counter in this block only
    count = count + 0 * i;
  } else {
    ;
  }
  i = i + 1;
}
sum * 100 + count
