function unsupportedDead() { const later = () => new Date(0); let n = 1; if (n > 2) { return new Date(0).getTime() + later().getTime(); } return 7; }
