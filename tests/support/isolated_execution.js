// Test-only isolation: namespaces share cljs.core/native tables. Load a fresh
// reference process for each selected mutation case; keep ordinary cases batched.
(() => {
  const {spawnSync} = require("node:child_process");
  const fs = require("node:fs"), path = require("node:path");
  const driver = fs.readFileSync(globalThis.lightExecutionDriver, "utf8");
  const indices = Array.from(lightCases, (_, i) => i);
  const prefixes = globalThis.lightIsolatedCasePrefixes;
  const isolated = i => !prefixes || prefixes.some(prefix => lightCaseMetadata[i][0].startsWith(prefix));
  const ordinary = indices.filter(i => !isolated(i));
  const batches = indices.filter(isolated).map(i => [i]);
  if (ordinary.length) batches.unshift(ordinary);
  const output = new Map();
  for (const selected of batches) {
    const script = `require(${JSON.stringify(path.join(process.cwd(), "reference.js"))});\n` +
      `const selected = ${JSON.stringify(selected)};\n` +
      `globalThis.lightCases = selected.map(i => lightCases[i]);\n` +
      `globalThis.lightCaseMetadata = selected.map(i => lightCaseMetadata[i]);\n` + driver;
    const result = spawnSync(process.execPath, ["-"], {input: script, encoding: "utf8", timeout: 30000, maxBuffer: 32 * 1024 * 1024});
    process.stderr.write(result.stderr ?? "");
    if (result.status !== 0 || result.error) throw new Error("isolated case process failed: " + selected.map(i => lightCaseMetadata[i][0]).join(", "));
    const rows = result.stdout.trimEnd().split("\n");
    if (rows.length !== selected.length) throw new Error("isolated case output omitted results");
    rows.forEach((row, index) => {
      const id = JSON.parse(row).id;
      if (id !== lightCaseMetadata[selected[index]][0] || output.has(id)) throw new Error("isolated case output changed identities");
      output.set(id, row);
    });
  }
  for (const [id] of lightCaseMetadata) process.stdout.write(output.get(id) + "\n");
})();
