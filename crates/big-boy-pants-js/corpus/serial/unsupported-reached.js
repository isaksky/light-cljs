function unsupportedReached() { function Point() {} try { const p = new Point(); return 1; } catch (e) { return -1; } }
