function directEvalSpread() { const args = ["1"]; try { return eval(...args) + 1; } catch (e) { return -1; } }
