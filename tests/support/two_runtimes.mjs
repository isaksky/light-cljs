// Two light-cljs runtimes on one page (docs/value-layout.md, section 1, "Two
// runtimes on one page"; light-cljs-1018). Usage:
//
//   node tests/support/two_runtimes.mjs --light BIN [--chromium NPM_TOOLS]
//
// Copies tests/fixtures/two-runtimes into a temporary directory and builds
// it there with BIN: `release app keyed flavor other` and `compile app other
// flavor` (the dev builds of app, of other, and of flavor, a second build of
// app's entry). It also writes `collide`, the release output of other
// with app's runtime key written for its own: two different outputs whose
// keys collide. Each scenario then loads its builds, in order, into one page:
// a fresh Node process, or with --chromium a fresh headless Chromium page
// (playwright-core from NPM_TOOLS, as `bb benchmarks/re-frame.bb setup`
// installs it). Before each load the page sets globalThis.twoRuntimesLoad,
// the load's label and options, and each build registers its runtime in
// globalThis.twoRuntimes (tests/fixtures/two-runtimes/src/two/shared.cljs).
// Once every build has loaded, the page asks each runtime what it answers
// about host values and its own state, and how it reads every runtime's
// values, its own included. The script prints each scenario's lines and
// exits with 1 when any differs from what the scenario expects.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import vm from "node:vm";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = path.join(here, "../fixtures/two-runtimes");

// What a runtime answers about the values of a runtime that recognises its
// own: all of them, the same value, each by kind. `box` is the count of the
// Box of the runtime that made it, which a load with extendBox changes.
const recognised = (box = 7) =>
  "vector:true,[2 1],true,same subvec:true,1,true,same map:true,1,true,same " +
  "hashMap:true,[5 1],true,same set:true,true,true,same list:true,1,true,same " +
  "lazy:true,[1 2],true,same range:true,[3 1],true,same " +
  'keyword:true,"k",true,same symbol:true,"sym",true,same ' +
  'entry:true,:a,true,same uuid:true,"00000000-0000-0000-0000-000000000001",true,same ' +
  `record:true,1,true,same box:true,${box},-,same atom:true,5,-,same greeter:true,"hi",-,same ` +
  'exInfo:true,["boom" {:a 1} {:b 2} [cljs.core/ExceptionInfo cljs.core/ExceptionInfo]],-,same holder:true,:found,true,same';

// What a runtime of the same build loaded again answers about them: every
// collection, keyword, symbol and uuid as its own, through the other copy's
// methods; the program's own record and Box types and the ExceptionInfo
// class are the other copy's classes, other types than its own (as a type
// defined again by a reload is). An ex-info of the other copy is not
// `instance?` of this copy's ExceptionInfo, but `ex-message`, `ex-data`,
// `ex-cause`, `Throwable->map` and printing answer for it as for its own
// (light-cljs-1044): it is recognised by its keyed kind.
const copy = (box = 7) =>
  recognised(box)
    .replace("record:true,1,true,same", "record:true,1,false,same")
    .replace("exInfo:true,", "exInfo:false,");

// What a runtime answers about another build's values: host objects, none
// of its kinds, and not equal to its own. A seq is a host iterable
// (`Symbol.iterator` is not keyed), which `first`, `vec` and `reduce`
// iterate as they iterate any host iterable. An ex-info is a host error,
// whose message `ex-message` reads.
const foreign =
  "vector:false,throws,false,object subvec:false,throws,false,object " +
  "map:false,nil,false,object hashMap:false,throws,false,object " +
  "set:false,false,false,object list:false,1,false,object " +
  "lazy:false,[1 2],false,object range:false,[3 1],false,object " +
  "keyword:false,throws,false,object symbol:false,throws,false,object " +
  'entry:false,throws,false,object uuid:false,"00000000-0000-0000-0000-000000000001",false,object ' +
  "record:false,nil,false,object box:false,throws,-,same " +
  'atom:false,throws,-,object greeter:false,throws,-,same exInfo:false,["boom" nil nil [js/Error]],-,object ' +
  "holder:false,nil,false,object";

const host = (greet, { count = "42", loads = 1, box = 7 } = {}) =>
  `date-count=${count} date-greet="${greet}" date-counted=${count !== "throws"} ` +
  `loads=${loads} box=${box}`;

const collision = (key) =>
  `Two light-cljs builds share the runtime key ${key}: give one a different :runtime-key`;

// Each scenario: the builds it loads, in order (a release chunk as an ES
// module, `copy` naming a second instance of one; a dev build's index.js;
// or a script, a dev build's unit evaluated again), and the lines it must print, the keys written as `<key of
// LABEL>`.
const scenarios = [
  {
    name: "two different builds",
    loads: [{ label: "app", module: "app/two.app.js" }, { label: "other", module: "other/two.other.js" }],
    expected: [
      "load app: ok",
      "load other: ok",
      `host app: ${host("app")}`,
      `host other: ${host("other", { count: "throws" })}`,
      `app sees app: ${recognised()}`,
      `app sees other: ${foreign}`,
      `other sees app: ${foreign}`,
      `other sees other: ${recognised()}`,
    ],
  },
  {
    name: "one build loaded twice",
    loads: [
      { label: "app", module: "app/two.app.js", extendBox: true },
      { label: "app again", module: "app/two.app.js", copy: 2 },
    ],
    expected: [
      "load app: ok",
      "load app again: ok",
      `host app: ${host("app", { box: 99 })}`,
      `host app again: ${host("app")}`,
      `app sees app: ${recognised(99)}`,
      `app sees app again: ${copy()}`,
      `app again sees app: ${copy(99)}`,
      `app again sees app again: ${recognised()}`,
    ],
  },
  {
    name: "one program with another :runtime-key",
    loads: [{ label: "app", module: "app/two.app.js" }, { label: "keyed", module: "keyed/two.app.js" }],
    expected: [
      "load app: ok",
      "load keyed: ok",
      `host app: ${host("app")}`,
      `host keyed: ${host("app")}`,
      `app sees app: ${recognised()}`,
      `app sees keyed: ${foreign}`,
      `keyed sees app: ${foreign}`,
      `keyed sees keyed: ${recognised()}`,
    ],
  },
  {
    name: "one program with another constant",
    loads: [{ label: "app", module: "app/two.app.js" }, { label: "flavor", module: "flavor/two.app.js" }],
    expected: [
      "load app: ok",
      "load flavor: ok",
      `host app: ${host("app")}`,
      `host flavor: ${host("flavor")}`,
      `app sees app: ${recognised()}`,
      `app sees flavor: ${foreign}`,
      `flavor sees app: ${foreign}`,
      `flavor sees flavor: ${recognised()}`,
    ],
  },
  {
    // The dev build's unit of two.app evaluated again, as a hot reload
    // does, registers again in the dev runtime, whose two.shared keeps its
    // state.
    name: "a dev build and a release build of one app, and a reload",
    loads: [
      { label: "dev", dev: "app-dev" },
      { label: "release", module: "app/two.app.js" },
      { label: "dev reload", script: "app-dev/two.app.js" },
    ],
    expected: [
      "load dev: ok",
      "load release: ok",
      "load dev reload: ok",
      `host dev: ${host("app", { loads: 2 })}`,
      `host release: ${host("app")}`,
      `host dev reload: ${host("app", { loads: 2 })}`,
      `dev sees dev: ${recognised()}`,
      `dev sees release: ${foreign}`,
      `dev sees dev reload: ${recognised()}`,
      `release sees dev: ${foreign}`,
      `release sees release: ${recognised()}`,
      `release sees dev reload: ${foreign}`,
      `dev reload sees dev: ${recognised()}`,
      `dev reload sees release: ${foreign}`,
      `dev reload sees dev reload: ${recognised()}`,
    ],
  },
  {
    // Two dev builds of two apps (light-cljs-1045): each has its own key,
    // runtime, client and definitions, so two.shared, which both define,
    // keeps each build's `defonce` state, and the first build's unit
    // evaluated again after the second loaded registers in the first's
    // runtime alone.
    name: "two dev builds of different apps, and a reload of the first",
    loads: [
      { label: "dev", dev: "app-dev" },
      { label: "other dev", dev: "other-dev" },
      { label: "dev reload", script: "app-dev/two.app.js" },
    ],
    expected: [
      "load dev: ok",
      "load other dev: ok",
      "load dev reload: ok",
      `host dev: ${host("app", { loads: 2 })}`,
      `host other dev: ${host("other", { count: "throws" })}`,
      `host dev reload: ${host("app", { loads: 2 })}`,
      `dev sees dev: ${recognised()}`,
      `dev sees other dev: ${foreign}`,
      `dev sees dev reload: ${recognised()}`,
      `other dev sees dev: ${foreign}`,
      `other dev sees other dev: ${recognised()}`,
      `other dev sees dev reload: ${foreign}`,
      `dev reload sees dev: ${recognised()}`,
      `dev reload sees other dev: ${foreign}`,
      `dev reload sees dev reload: ${recognised()}`,
    ],
  },
  {
    // Two dev builds of one entry, two builds of light-cljs.edn: the same
    // namespaces, kept apart by their keys, and a reload of the second.
    name: "two dev builds of one app, and a reload of the second",
    loads: [
      { label: "dev", dev: "app-dev" },
      { label: "flavor dev", dev: "flavor-dev" },
      { label: "flavor reload", script: "flavor-dev/two.app.js" },
    ],
    expected: [
      "load dev: ok",
      "load flavor dev: ok",
      "load flavor reload: ok",
      `host dev: ${host("app")}`,
      `host flavor dev: ${host("app", { loads: 2 })}`,
      `host flavor reload: ${host("app", { loads: 2 })}`,
      `dev sees dev: ${recognised()}`,
      `dev sees flavor dev: ${foreign}`,
      `dev sees flavor reload: ${foreign}`,
      `flavor dev sees dev: ${foreign}`,
      `flavor dev sees flavor dev: ${recognised()}`,
      `flavor dev sees flavor reload: ${recognised()}`,
      `flavor reload sees dev: ${foreign}`,
      `flavor reload sees flavor dev: ${recognised()}`,
      `flavor reload sees flavor reload: ${recognised()}`,
    ],
  },
  {
    // Two dev builds loading at once, the first of whose units throws while
    // the second's loader is pending: the failure is the first build's
    // alone (the Sol review of light-cljs-1045). A page reports the throw
    // as an uncaught error.
    name: "two dev builds loading at once, the first failing",
    loads: [{ label: "dev", together: ["broken-dev", "app-dev"] }],
    pageErrors: ["broken dev failed"],
    expected: [
      "load dev broken-dev: broken dev failed",
      "load dev app-dev: ok",
      `host dev: ${host("app")}`,
      `dev sees dev: ${recognised()}`,
    ],
  },
  {
    name: "a release build and then a dev build of one app",
    loads: [{ label: "release", module: "app/two.app.js" }, { label: "dev", dev: "app-dev" }],
    expected: [
      "load release: ok",
      "load dev: ok",
      `host release: ${host("app")}`,
      `host dev: ${host("app")}`,
      `release sees release: ${recognised()}`,
      `release sees dev: ${foreign}`,
      `dev sees release: ${foreign}`,
      `dev sees dev: ${recognised()}`,
    ],
  },
  {
    name: "two different outputs whose keys collide",
    loads: [
      { label: "app", module: "app/two.app.js" },
      { label: "collide", module: "collide/two.other.js" },
      { label: "other", module: "other/two.other.js" },
    ],
    expected: [
      "load app: ok",
      `load collide: ${collision("<key of app>")}`,
      "load other: ok",
      `host app: ${host("app")}`,
      `host other: ${host("other", { count: "throws" })}`,
      `app sees app: ${recognised()}`,
      `app sees other: ${foreign}`,
      `other sees app: ${foreign}`,
      `other sees other: ${recognised()}`,
    ],
  },
];

/** The page: load each of `loads` in order, then ask every runtime. Runs in
 *  Node and, serialized by Playwright, in Chromium, so it names nothing
 *  outside itself; in Node, globalThis.twoRuntimesLoader loads. */
async function page(loads) {
  const lines = [];
  // One host object every runtime's values hold (`holder`).
  globalThis.twoRuntimesShared = {};
  for (const load of loads) {
    globalThis.twoRuntimesLoad = { label: load.label, extendBox: load.extendBox === true };
    if (load.together) {
      // Dev builds whose index.js all run, in order, before any of their
      // files, as script elements with `async = false` do: each build's
      // loader is pending while the others' units run. Each settles as its
      // own build did; the builds register under the load's label.
      const loaded = globalThis.twoRuntimesLoader
        ? await globalThis.twoRuntimesLoader(load)
        : await Promise.all(load.together.map((dir) => new Promise((resolve, reject) => {
          const script = document.createElement("script");
          script.src = new URL(`${dir}/index.js`, location.href).href;
          script.async = false;
          script.onload = () => resolve({ promise: globalThis.__lightDevLoaded });
          script.onerror = () => reject(new Error(`cannot load ${script.src}`));
          document.head.append(script);
        })));
      const settled = await Promise.allSettled(loaded.map((each) => each.promise));
      settled.forEach((result, at) => lines.push(`load ${load.label} ${load.together[at]}: ` +
        (result.status === "fulfilled" ? "ok" : result.reason?.message ?? result.reason)));
      continue;
    }
    try {
      if (globalThis.twoRuntimesLoader) {
        await globalThis.twoRuntimesLoader(load);
      } else if (load.dev) {
        await new Promise((resolve, reject) => {
          const script = document.createElement("script");
          script.src = new URL(`${load.dev}/index.js`, location.href).href;
          script.onload = resolve;
          script.onerror = () => reject(new Error(`cannot load ${script.src}`));
          document.head.append(script);
        });
        await globalThis.__lightDevLoaded;
      } else if (load.script) {
        await new Promise((resolve, reject) => {
          const script = document.createElement("script");
          script.src = new URL(load.script, location.href).href;
          script.onload = resolve;
          script.onerror = () => reject(new Error(`cannot load ${script.src}`));
          document.head.append(script);
        });
      } else {
        await import(new URL(`${load.module}?copy=${load.copy ?? 1}`, location.href).href);
      }
      lines.push(`load ${load.label}: ok`);
    } catch (error) {
      lines.push(`load ${load.label}: ${error?.message ?? error}`);
    }
  }
  const runtimes = globalThis.twoRuntimes ?? [];
  for (const runtime of runtimes) lines.push(`host ${runtime.label}: ${runtime.host()}`);
  for (const runtime of runtimes) {
    for (const other of runtimes) lines.push(`${runtime.label} sees ${other.label}: ${runtime.observe(other.values)}`);
  }
  return lines;
}

/** One scenario's page in this Node process: the builds under `out`. */
async function nodePage(out, loads) {
  const require = createRequire(path.join(out, "page.cjs"));
  globalThis.twoRuntimesLoader = async (load) => {
    if (load.together) {
      // Node requires each build's files as its index.js runs.
      return load.together.map((dir) => {
        require(path.join(out, dir, "index.js"));
        return { promise: globalThis.__lightDevLoaded };
      });
    }
    if (load.dev) {
      require(path.join(out, load.dev, "index.js"));
      await globalThis.__lightDevLoaded;
    } else if (load.script) {
      const file = path.join(out, load.script);
      vm.runInThisContext(fs.readFileSync(file, "utf8"), { filename: file });
    } else {
      await import(`${pathToFileURL(path.join(out, load.module)).href}?copy=${load.copy ?? 1}`);
    }
  };
  return page(loads);
}

function option(args, flag) {
  const at = args.indexOf(flag);
  return at < 0 ? undefined : args[at + 1];
}

function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed (${result.status}):\n${result.stderr}${result.stdout}`);
  }
  return result.stdout;
}

/** The runtime key and hash a release output registers. */
function writtenKey(text) {
  const match = /\("\$([0-9a-z]{8})",\s*"([0-9a-f]{16})"\)/.exec(text);
  if (!match) throw new Error("the output registers no runtime key");
  return { key: match[1], hash: match[2] };
}

/** Builds the fixture with `light` under `dir`; returns the output directory
 *  and the key of each release build, by build. */
function build(light, dir) {
  const project = path.join(dir, "project");
  fs.cpSync(fixture, project, { recursive: true });
  run(light, ["release", "app", "keyed", "flavor", "other"], project);
  run(light, ["compile", "app", "other", "flavor"], project);
  const out = path.join(project, "out");
  const keys = {};
  for (const [label, file] of [["app", "app/two.app.js"], ["keyed", "keyed/two.app.js"],
    ["flavor", "flavor/two.app.js"], ["other", "other/two.other.js"]]) {
    keys[label] = writtenKey(fs.readFileSync(path.join(out, file), "utf8")).key;
  }
  // other, with app's key written for its own.
  const other = fs.readFileSync(path.join(out, "other/two.other.js"), "utf8");
  fs.mkdirSync(path.join(out, "collide"));
  fs.writeFileSync(path.join(out, "collide/two.other.js"), other.split(`$${keys.other}`).join(`$${keys.app}`));
  // other's dev build, whose two.other unit throws as it runs.
  fs.cpSync(path.join(out, "other-dev"), path.join(out, "broken-dev"), { recursive: true });
  fs.writeFileSync(path.join(out, "broken-dev/two.other.js"), 'throw new Error("broken dev failed");\n');
  fs.writeFileSync(path.join(out, "index.html"), "<!doctype html><title>two runtimes</title>\n");
  return { out, keys };
}

async function chromiumPages(out, npmTools, scenarios) {
  const browserKit = await import(pathToFileURL(path.join(here, "../../benchmarks/re-frame-browser.mjs")).href);
  const server = await browserKit.serve(out);
  const browser = await browserKit.launch(npmTools);
  try {
    const results = [];
    for (const scenario of scenarios) {
      // A console error or an uncaught exception is a line no scenario
      // expects, but for the dev client's request for the event stream of
      // the dev server this static server is not, and an error the scenario
      // throws on purpose (`pageErrors`).
      const errors = [];
      const expected = (text) => (scenario.pageErrors ?? []).some((error) => text.includes(error));
      const context = await browser.newContext();
      const tab = await context.newPage();
      tab.on("console", (message) => {
        const events = /\/events$/.test(message.location()?.url ?? "");
        if (message.type() === "error" && !events && !expected(message.text())) {
          errors.push(`console: ${message.text().split("\n")[0]}`);
        }
      });
      tab.on("pageerror", (error) => {
        if (!expected(error.message)) errors.push(`exception: ${error.message}`);
      });
      try {
        await tab.goto(`${server.url}index.html`);
        results.push([...(await tab.evaluate(page, scenario.loads)), ...errors]);
      } finally {
        await context.close();
      }
    }
    return results;
  } finally {
    await browser.close();
    await server.close();
  }
}

async function main(args) {
  if (args[0] === "--node-page") {
    process.stdout.write(JSON.stringify(await nodePage(args[1], JSON.parse(args[2]))));
    return 0;
  }
  const light = option(args, "--light");
  const npmTools = option(args, "--chromium");
  if (!light) throw new Error("usage: node tests/support/two_runtimes.mjs --light BIN [--chromium NPM_TOOLS]");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "light-two-runtimes-"));
  try {
    const { out, keys } = build(path.resolve(light), dir);
    const results = npmTools
      ? await chromiumPages(out, path.resolve(npmTools), scenarios)
      : scenarios.map((scenario) =>
          JSON.parse(run(process.execPath, [fileURLToPath(import.meta.url), "--node-page", out, JSON.stringify(scenario.loads)], out)));
    const host = npmTools ? "headless Chromium" : `Node ${process.version}`;
    let failed = 0;
    scenarios.forEach((scenario, index) => {
      const lines = results[index].map((line) =>
        Object.entries(keys).reduce((text, [label, key]) => text.split(key).join(`<key of ${label}>`), line));
      const ok = JSON.stringify(lines) === JSON.stringify(scenario.expected);
      failed += ok ? 0 : 1;
      console.log(`${ok ? "ok" : "FAILED"}: ${scenario.name} (${host})`);
      if (!ok) {
        const width = Math.max(lines.length, scenario.expected.length);
        for (let i = 0; i < width; i++) {
          const [expected, actual] = [scenario.expected[i] ?? "", lines[i] ?? ""];
          if (actual === expected) continue;
          // The words that differ, for the long lines of a runtime's view.
          const [want, got] = [expected.split(" "), actual.split(" ")];
          const words = want.length === got.length && want[0] === got[0] && want[1] === got[1]
            ? want.flatMap((word, at) => (word === got[at] ? [] : [`${word} -> ${got[at]}`]))
            : [`${expected}\n      -> ${actual}`];
          console.log(`  line ${i + 1} (${want.slice(0, 3).join(" ")}): ${words.join("; ")}`);
        }
      }
    });
    console.log(`${scenarios.length - failed} of ${scenarios.length} scenarios as expected in ${host}`);
    return failed === 0 ? 0 : 1;
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

process.exitCode = await main(process.argv.slice(2));
