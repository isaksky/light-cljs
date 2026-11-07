function stringToNumber() {
  const s = "  0x10\n"; const t = " 12345678 "; const u = "2"; let i = "41"; i++;
  return -s === -16 && +t === 12345678 && t - 1 === 12345677 && t * u === 24691356 && u < 16 && s > u * 1 && s == 16 && "1e3" * u === 2000 && i === 42 && " 0.5 " < true;
}
