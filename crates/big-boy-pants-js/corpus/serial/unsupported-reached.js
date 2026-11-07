function unsupportedReached() { function Point() {} try { const p = new Point(); return p instanceof Point ? 1 : 0; } catch (e) { return -1; } }
