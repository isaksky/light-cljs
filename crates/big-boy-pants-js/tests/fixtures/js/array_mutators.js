// Synthetic fixture, not light-cljs output (clj-quickjs-296).
let a = [1, 2, 3];
let ok = a.pop() === 3 && a.shift() === 1;
ok = ok && a.unshift(4, 5) === 3 && a.join() === '4,5,2';
let removed = a.splice(1, 1, 6, 7);
ok = ok && removed.join() === '5' && a.join() === '4,6,7,2';
ok = ok && a.reverse() === a && a.join() === '2,7,6,4';
ok = ok && a.fill(8, 1, 3) === a && a.join() === '2,8,8,4';
ok = ok && a.copyWithin(1, 0, 3) === a && a.join() === '2,2,8,8';
ok;
