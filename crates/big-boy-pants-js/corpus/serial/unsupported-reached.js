function unsupportedReached() { function Point() {} try { const p = new Point(); for (const q of [p]) { return q instanceof Point ? 1 : 0; } return 0; } catch (e) { return -1; } }
