// bbp-authored conformance case.
// Derives from: none (harness contract: the names the QuickJS driver uses
// internally are free for a case's own global declarations). Completes with 15.
let path = 1, mode = 2, line = 3;
function canonical() { return 4; }
function thrown() { return 5; }
path + mode + line + canonical() + thrown();
