// bbp-authored conformance case.
// Derives from: none (light-cljs-847: strings built in a loop, of which
// only the last are kept. Each iteration makes new Latin-1 and UTF-16
// strings (a number's text and two concatenations); together they pass the
// Goose VM's 1,048,576-unit job limit, so the job reclaims the unreachable
// strings and their code units while it runs). Completes with
// "item-149999|€149999|2477780".
(function () {
  let narrow = "", wide = "", total = 0;
  for (let i = 0; i < 150000; i++) {
    narrow = "item-" + i;
    wide = "€" + i;
    total += narrow.length + wide.length;
  }
  return narrow + "|" + wide + "|" + total;
})();
