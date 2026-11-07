function indirectEval() { try { return (0, eval)("1") + 1; } catch (e) { return -1; } }
