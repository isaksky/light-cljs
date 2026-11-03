function control() {
  let n = 0; let sum = 0;
  outer: while (n < 8) { n = n + 1; if (n < 3) continue outer; sum = sum + n; }
  return sum === 33 && (0 || 7) === 7 && (4 && 9) === 9 && (null ?? 3) === 3 && (false ?? 8) === false && ("" || "yes") === "yes" && ("a" && "b") === "b";
}
