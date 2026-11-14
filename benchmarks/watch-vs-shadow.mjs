// The browser side of benchmarks/watch-vs-shadow.bb: opens one tool's dev page
// in headless Chromium while that tool's watch runs, then saves edits and
// times each one. Run by the bb kit with one argument, a JSON file:
//
//   {tool: "shadow"|"light", url, events, npmTools, outDir, rootPid,
//    ready: JS expression true once the app has rendered,
//    scenarios: [{name, file, text, edits: [text...], tokens: [token...],
//                 probe: {dom: true | call: "ns/name", args | def: "ns/name",
//                         original, offset, modulus}}]}
// text is the file's original content, which is written back after the
// scenario's edits; original is a string the probe shows before any edit.
//
// For each edit it writes the file, takes the file's modification time as
// time zero, and waits for three things:
//   output  the newest modification time under outDir after time zero, read
//           once the page runs the new code (each tool writes its files
//           before it pushes the reload);
//   message the reload message reaching a listener on this machine: shadow's
//           `cljs-build-complete` websocket frame to the page, or light-cljs's
//           `load` server-sent event on a second /events connection;
//   running the page evaluating a probe that is true only once the edit's new
//           code ran: the page text (dom), the result of calling the
//           function the edit changes with ARGS (call) as the page's
//           namespace objects (shadow-cljs) or definition registry
//           (light-cljs) hold it, or that function's source text (def).
//           A call's result shows the token, or, with offset, is a number
//           whose constant OFFSET became the token: before + token - offset,
//           modulo MODULUS.
// It also reads the watch's process tree's CPU time before and after each
// edit. Prints {pageLoadMs, edits: [{scenario, i, load, mtime, output,
// message, running, cpuUser, cpuSys}]} as JSON; times are milliseconds since
// the edit's modification time.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { launch, openApp } from "./re-frame-browser.mjs";

const config = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const now = () => performance.timeOrigin + performance.now();

function cpuSeconds(text) {
  // ps prints [[dd-]hh:]mm:ss.ss
  const parts = text.split(/[-:]/).map(Number);
  return parts.reduce((acc, x) => acc * 60 + x, 0);
}

/** User and system CPU seconds of rootPid and its descendants. */
function treeCpu(rootPid) {
  const rows = execFileSync("ps", ["-A", "-o", "pid=,ppid=,utime=,stime="], { encoding: "utf8" })
    .trim().split("\n").map((l) => l.trim().split(/\s+/));
  const children = new Map();
  for (const [pid, ppid] of rows) children.set(ppid, [...(children.get(ppid) ?? []), pid]);
  const tree = new Set([String(rootPid)]);
  for (const pid of tree) for (const c of children.get(pid) ?? []) tree.add(c);
  let user = 0, sys = 0;
  for (const [pid, , u, s] of rows) if (tree.has(pid)) { user += cpuSeconds(u); sys += cpuSeconds(s); }
  return { user, sys };
}

function newestMtime(dir, after) {
  let newest = null;
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else {
        const m = fs.statSync(p).mtimeMs;
        if (m > after && (newest === null || m > newest)) newest = m;
      }
    }
  };
  walk(dir);
  return newest;
}

// Reload messages, as {at} in arrival order; each edit waits for the first
// one after its time zero.
const messages = [];
async function listenLight(url) {
  const res = await fetch(url);
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  (async () => {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) return;
      const at = now();
      buffer += decoder.decode(value, { stream: true });
      let i;
      while ((i = buffer.indexOf("\n\n")) >= 0) {
        const event = buffer.slice(0, i);
        buffer = buffer.slice(i + 2);
        const data = event.split("\n").filter((l) => l.startsWith("data:")).map((l) => l.slice(5)).join("\n");
        if (!data) continue;
        const msg = JSON.parse(data);
        if (msg.op === "load" && !msg.replay) messages.push({ at });
      }
    }
  })().catch(() => {});
}

async function waitFor(predicate, what, ms) {
  const deadline = Date.now() + ms;
  for (;;) {
    const v = await predicate();
    if (v) return v;
    if (Date.now() > deadline) throw new Error(`timed out after ${ms} ms waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 2));
  }
}

const probeSource = {
  // shadow-cljs dev builds keep each namespace as a global object path.
  shadow: (def) => {
    const [ns, name] = def.split("/");
    const path = [...ns.split("."), name].map((s) => s.replace(/-/g, "_").replace(/!/g, "_BANG_").replace(/\?/g, "_QMARK_"));
    return `${JSON.stringify(path)}.reduce((o, k) => o?.[k], globalThis)`;
  },
  light: (def) => `globalThis.__lightApplicationRoots?.[${JSON.stringify(def)}]`,
};

const browser = await launch(config.npmTools);
const errors = [];
let result;
try {
  if (config.tool === "light") await listenLight(config.events);
  const start = now();
  const context = await browser.newContext();
  const page = await context.newPage();
  page.on("pageerror", (e) => errors.push(`exception: ${e.message}`));
  page.on("console", (m) => { if (m.type() === "error") errors.push(`console: ${m.text().split("\n")[0]}`); });
  page.on("websocket", (ws) => ws.on("framereceived", (f) => {
    const at = now();
    if (String(f.payload).includes("cljs-build-complete")) messages.push({ at });
  }));
  await page.goto(config.url, { timeout: 600000 });
  await page.waitForFunction(config.ready, null, { timeout: 600000, polling: 20 });
  const pageLoadMs = now() - start;
  const edits = [];
  for (const sc of config.scenarios) {
    const p = sc.probe;
    const value = p.dom ? "document.body.innerText"
      : p.call && p.offset !== undefined ? `${probeSource[config.tool](p.call)}(...${JSON.stringify(p.args ?? [])})`
      : p.call ? `String(${probeSource[config.tool](p.call)}(...${JSON.stringify(p.args ?? [])}))`
      : `String(${probeSource[config.tool](p.def)})`;
    const before = await page.evaluate(value);
    let shows;
    if (p.offset !== undefined) {
      if (typeof before !== "number") throw new Error(`${sc.name}: the probe gives ${before}, not a number`);
      const m = p.modulus;
      shows = (token) => `(${value}) === ${(((before - p.offset + Number(token)) % m) + m) % m}`;
    } else {
      if (!String(before).includes(p.original)) {
        throw new Error(`${sc.name}: the probe does not show ${JSON.stringify(p.original)} before the first edit: ${String(before).slice(0, 300)}`);
      }
      shows = (token) => `String(${value}).includes(${JSON.stringify(token)})`;
    }
    for (let i = 0; i < sc.edits.length; i++) {
      const token = sc.tokens[i];
      const load = os.loadavg()[0];
      const cpu0 = treeCpu(config.rootPid);
      fs.writeFileSync(sc.file, sc.edits[i]);
      const mtime = fs.statSync(sc.file).mtimeMs;
      const msg = await waitFor(() => messages.find((m) => m.at >= mtime), `${sc.name} edit ${i}: reload message`, config.timeoutMs);
      const running = await waitFor(async () => {
        const at = await page.evaluate(`(${shows(token)}) ? performance.timeOrigin + performance.now() : 0`);
        return at || null;
      }, `${sc.name} edit ${i}: the page to run the new code`, config.timeoutMs);
      const cpu1 = treeCpu(config.rootPid);
      const output = newestMtime(config.outDir, mtime);
      edits.push({ scenario: sc.name, i, load, mtime,
                   output: output === null ? null : output - mtime,
                   message: msg.at - mtime, running: running - mtime,
                   cpuUser: cpu1.user - cpu0.user, cpuSys: cpu1.sys - cpu0.sys });
      process.stderr.write(`${config.tool} ${sc.name} ${i}: output ${output === null ? "-" : (output - mtime).toFixed(0)} message ${(msg.at - mtime).toFixed(0)} running ${(running - mtime).toFixed(0)} ms (load ${load.toFixed(2)})\n`);
      await new Promise((r) => setTimeout(r, config.pauseMs ?? 1000));
    }
    // Put the file back and wait until the page runs the original again,
    // so that this rebuild never overlaps the next scenario's edits.
    const last = sc.tokens[sc.tokens.length - 1];
    fs.writeFileSync(sc.file, sc.text);
    const restored = fs.statSync(sc.file).mtimeMs;
    await waitFor(() => messages.find((m) => m.at >= restored), `${sc.name}: reload after restoring`, config.timeoutMs);
    await waitFor(async () => !(await page.evaluate(shows(last))), `${sc.name}: the page to run the original`, config.timeoutMs);
    process.stderr.write(`${config.tool} ${sc.name}: restored\n`);
    await new Promise((r) => setTimeout(r, config.pauseMs ?? 1000));
  }
  result = { pageLoadMs, edits, errors };
} finally {
  await browser.close();
}
console.log(JSON.stringify(result));
// The light-cljs event stream keeps the process alive.
process.exit(0);
