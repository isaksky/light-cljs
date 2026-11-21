// Measures in-process startup phases of a re-frame-200k release build in Node:
//   node benchmarks/gap-bench/startup-probe.mjs --tool light|shadow --file JS --project-dir DIR [--seed 42]
//
// Sets up JSDOM (and globalThis.lightCljsPackages for light-cljs) before loading
// the bundle so both compilers start on equal footing and neither pays JSDOM
// construction inside firstIterMs. Prints one JSON line:
//   {tool, instrumented, compileMs, runtimeMs, topLevelMs, loadTotalMs, firstIterMs, totalMs, checksum}
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { createRequire } from "node:module";
import { performance } from "node:perf_hooks";

function arg(flag, dflt = null) {
  const i = process.argv.indexOf(flag);
  return i >= 0 && i + 1 < process.argv.length ? process.argv[i + 1] : dflt;
}

/** Find the index right after the nested program boundary `return(()=>{` in a
 *  light-cljs ES module (`export default (()=>{<units>return(()=>{<program>})()})();`).
 *  Matches the exact emitter truthiness prologue right after `return(()=>{` first
 *  (immune to regex literals inside runtime units), falling back to the first
 *  `return(()=>{` in the runtime prologue window. */
function findNestedProgramBoundary(src, bodyStart) {
  const windowEnd = Math.min(src.length, bodyStart + 600_000);
  const slice = src.slice(bodyStart, windowEnd);
  const truthyMatch = /return\s*\(\(\)=>\{(?=const\s+[A-Za-z_$][\w$]*=value=>value!=null&&value!==false)/.exec(slice);
  if (truthyMatch) {
    return bodyStart + truthyMatch.index + truthyMatch[0].length;
  }
  const simpleMatch = /return\s*\(\(\)=>\{/.exec(slice);
  if (simpleMatch) {
    return bodyStart + simpleMatch.index + simpleMatch[0].length;
  }
  return -1;
}

function instrumentLight(src) {
  const marker = "export default (()=>{";
  const idx = src.indexOf(marker);
  if (idx < 0) return { code: src, instrumented: false };
  const bodyStart = idx + marker.length;
  const progIdx = findNestedProgramBoundary(src, bodyStart);
  const stampStart = "globalThis.__gapTStart=performance.now();";
  if (progIdx > bodyStart) {
    const stampProg = "globalThis.__gapTProg=performance.now();";
    const code =
      src.slice(0, bodyStart) +
      stampStart +
      src.slice(bodyStart, progIdx) +
      stampProg +
      src.slice(progIdx);
    return { code, instrumented: "nested" };
  }
  // Fallback for non-nested factory output: still separates compile vs. load.
  const code = src.slice(0, bodyStart) + stampStart + src.slice(bodyStart);
  return { code, instrumented: "factory" };
}

function instrumentShadow(src, seed) {
  const clean = src.startsWith("#!") ? src.slice(src.indexOf("\n") + 1) : src;
  const open = "(function(){";
  const idx = clean.indexOf(open);
  const argvToken = "process.argv.slice(2)";
  const argvIdx = clean.lastIndexOf(argvToken);
  if (idx < 0 || argvIdx < 0) return { code: clean, instrumented: false };
  const stampStart = "globalThis.__gapTStart=performance.now();";
  const argvRepl = `((globalThis.__gapTMain=performance.now()),[${JSON.stringify(String(seed))},"1"])`;
  const code =
    clean.slice(0, idx + open.length) +
    stampStart +
    clean.slice(idx + open.length, argvIdx) +
    argvRepl +
    clean.slice(argvIdx + argvToken.length);
  return { code, instrumented: true };
}

async function run() {
  const tool = arg("--tool");
  const file = path.resolve(arg("--file"));
  const projectDir = path.resolve(arg("--project-dir"));
  const seed = arg("--seed", "42");
  if (!tool || !file || !projectDir) {
    throw new Error("Usage: node startup-probe.mjs --tool light|shadow --file JS --project-dir DIR [--seed 42]");
  }

  // Prepare JSDOM + packages before timing either compiler.
  const req = createRequire(path.join(projectDir, "package.json"));
  const { JSDOM } = req("jsdom");
  const dom = new JSDOM("<!doctype html><html><body></body></html>");
  globalThis.window = dom.window;
  globalThis.document = dom.window.document;
  if (tool === "light") {
    globalThis.lightCljsPackages = {
      react: req("react"),
      "react-dom": req("react-dom"),
      "react-dom/client": req("react-dom/client"),
    };
  }

  const origLog = console.log;
  const logged = [];
  console.log = (...xs) => {
    logged.push(xs.map(String).join(" "));
  };

  const src = fs.readFileSync(file, "utf8");
  const tmpName = `.gap-startup-probe-${process.pid}-${Date.now()}.${tool === "light" ? "mjs" : "cjs"}`;
  const tmpFile = path.join(projectDir, tmpName);

  try {
    if (tool === "light") {
      const { code, instrumented } = instrumentLight(src);
      fs.writeFileSync(tmpFile, code);
      const t0 = performance.now();
      const mod = await import(pathToFileURL(tmpFile).href);
      const tLoaded = performance.now();
      mod.default(String(seed), "1");
      const tDone = performance.now();

      const tStart = globalThis.__gapTStart;
      const tProg = globalThis.__gapTProg;
      const compileMs = tStart !== undefined ? tStart - t0 : null;
      const runtimeMs = tStart !== undefined && tProg !== undefined ? tProg - tStart : null;
      const topLevelMs =
        tProg !== undefined ? tLoaded - tProg : tStart !== undefined ? tLoaded - tStart : null;
      const loadTotalMs = tLoaded - t0;
      const firstIterMs = tDone - tLoaded;
      const totalMs = tDone - t0;
      console.log = origLog;
      process.stdout.write(
        JSON.stringify({
          tool,
          instrumented,
          compileMs,
          runtimeMs,
          topLevelMs,
          loadTotalMs,
          firstIterMs,
          totalMs,
          checksum: logged.join("\n").trim(),
        }) + "\n"
      );
    } else if (tool === "shadow") {
      const { code, instrumented } = instrumentShadow(src, seed);
      fs.writeFileSync(tmpFile, code);
      const t0 = performance.now();
      req(tmpFile);
      const tDone = performance.now();

      const tStart = globalThis.__gapTStart;
      const tMain = globalThis.__gapTMain;
      const compileMs = tStart !== undefined ? tStart - t0 : null;
      const topLevelMs = tStart !== undefined && tMain !== undefined ? tMain - tStart : null;
      const loadTotalMs = tMain !== undefined ? tMain - t0 : null;
      const firstIterMs = tMain !== undefined ? tDone - tMain : null;
      const totalMs = tDone - t0;
      console.log = origLog;
      process.stdout.write(
        JSON.stringify({
          tool,
          instrumented,
          compileMs,
          runtimeMs: null,
          topLevelMs,
          loadTotalMs,
          firstIterMs,
          totalMs,
          checksum: logged.join("\n").trim(),
        }) + "\n"
      );
    } else {
      throw new Error(`Unknown --tool: ${tool}`);
    }
  } finally {
    console.log = origLog;
    try {
      fs.unlinkSync(tmpFile);
    } catch {}
  }
}

run().catch((err) => {
  process.stderr.write((err?.stack ?? String(err)) + "\n");
  process.exit(1);
});
