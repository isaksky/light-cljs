function directEvalReplaced() { const original = globalThis.eval; globalThis.eval = function (x) { return x + 1; }; const n = eval(1); globalThis.eval = original; return n; }
