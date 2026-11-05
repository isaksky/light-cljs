function strings() { const s = "😀\ud800"; return s.length === 3 && s[0] === "\ud83d" && s[1] === "\ude00" && s[2] === "\ud800" && s[3] === undefined; }
