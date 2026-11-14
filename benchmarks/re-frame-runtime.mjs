// How fast one compiler's TodoMVC build runs in headless Chromium, in the
// spirit of js-framework-benchmark. Run by `bb benchmarks/re-frame.bb runtime`,
// which alternates the compilers and summarizes; usable alone:
//
//   node benchmarks/re-frame-runtime.mjs --dir DIR --npm-tools DIR [--todos 1000]
//
// DIR is a `bb benchmarks/re-frame.bb bundle todomvc` directory. One run: a
// fresh browser (so no compiled-code cache survives from another run), the
// page loaded, then, in the page, through the DOM events a user's input sends:
// add TODOS todos one after another, toggle all, edit the middle one, clear
// completed. Each step's time runs from the first event to the frame after the
// DOM shows the step's result, by performance.now() in the page; the Chrome
// DevTools Protocol's Performance metrics give the step's script, layout and
// style-recalculation time and the JavaScript heap after a garbage collection.
// Prints one JSON object:
//   {steps: {load, add, toggleAll, edit, clearCompleted: {ms, scriptMs, layoutMs, recalcStyleMs}},
//    heapAfterLoad, heapAfterAdd, errors}
// where load's ms is navigation start to the new-todo input in the DOM.
import { serve, launch } from "./re-frame-browser.mjs";

// Installed before the page's scripts: the time the app's input appears.
function recordInteractive() {
  new MutationObserver((_, observer) => {
    if (document.getElementById("new-todo")) {
      window.__interactive = performance.now();
      observer.disconnect();
    }
  }).observe(document, { childList: true, subtree: true });
}

// In the page: run STEP's events and resolve with the milliseconds until the
// frame after DONE() first holds. DONE is checked after every DOM mutation.
export function inPage({ step, todos }) {
  const setValue = (input, value) => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const enter = (input) =>
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", keyCode: 13, which: 13, bubbles: true }));
  const items = () => document.querySelectorAll("#todo-list li");
  const middle = Math.floor(todos / 2);
  const steps = {
    add: [() => {
      const input = document.getElementById("new-todo");
      for (let i = 0; i < todos; i++) { setValue(input, `todo ${i}`); enter(input); }
    }, () => items().length === todos],
    toggleAll: [() => document.getElementById("toggle-all").click(),
      () => { const li = items(); return li.length === todos && [...li].every((e) => e.classList.contains("completed")); }],
    edit: [() => {
      items()[middle].querySelector("label").dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
      return new Promise((resolve) => {
        const poll = () => {
          const input = items()[middle].querySelector("input.edit");
          if (!input) return requestAnimationFrame(poll);
          setValue(input, "edited");
          enter(input);
          resolve();
        };
        poll();
      });
    }, () => { const li = items()[middle]; return li && !li.classList.contains("editing") && li.querySelector("label").textContent === "edited"; }],
    clearCompleted: [() => document.getElementById("clear-completed").click(), () => items().length === 0],
  };
  const [act, done] = steps[step];
  return new Promise((resolve, reject) => {
    const start = performance.now();
    let finished = false;
    const finish = () => {
      if (finished || !done()) return;
      finished = true;
      observer.disconnect();
      // The frame after the DOM change: rendered, then a task after it.
      requestAnimationFrame(() => setTimeout(() => resolve(performance.now() - start)));
    };
    const observer = new MutationObserver(finish);
    observer.observe(document.body, { childList: true, subtree: true, attributes: true, characterData: true });
    setTimeout(() => { if (!finished) { observer.disconnect(); reject(new Error(`${step} did not finish in 120 s`)); } }, 120000);
    Promise.resolve(act()).then(finish, reject);
  });
}

async function metrics(cdp) {
  const { metrics } = await cdp.send("Performance.getMetrics");
  return Object.fromEntries(metrics.map((m) => [m.name, m.value]));
}

async function heap(cdp) {
  await cdp.send("HeapProfiler.collectGarbage");
  return (await metrics(cdp)).JSHeapUsedSize;
}

const durations = (before, after) => ({
  scriptMs: (after.ScriptDuration - before.ScriptDuration) * 1000,
  layoutMs: (after.LayoutDuration - before.LayoutDuration) * 1000,
  recalcStyleMs: (after.RecalcStyleDuration - before.RecalcStyleDuration) * 1000,
});

export async function runTodomvc({ dir, npmTools, todos = 1000 }) {
  const server = await serve(dir);
  const browser = await launch(npmTools);
  const errors = [];
  try {
    const context = await browser.newContext();
    const page = await context.newPage();
    page.on("console", (m) => { if (m.type() === "error") errors.push(`console: ${m.text().split("\n")[0]}`); });
    page.on("pageerror", (e) => errors.push(`exception: ${e.message}`));
    await page.addInitScript(recordInteractive);
    const cdp = await context.newCDPSession(page);
    await cdp.send("Performance.enable");
    await page.goto(server.url, { waitUntil: "load" });
    await page.waitForFunction(() => window.__interactive !== undefined, null, { timeout: 30000 });
    const loaded = await metrics(cdp);
    const steps = {
      load: { ms: await page.evaluate(() => window.__interactive), scriptMs: loaded.ScriptDuration * 1000,
              layoutMs: loaded.LayoutDuration * 1000, recalcStyleMs: loaded.RecalcStyleDuration * 1000 },
    };
    const heapAfterLoad = await heap(cdp);
    let heapAfterAdd;
    for (const step of ["add", "toggleAll", "edit", "clearCompleted"]) {
      const before = await metrics(cdp);
      const ms = await page.evaluate(inPage, { step, todos });
      steps[step] = { ms, ...durations(before, await metrics(cdp)) };
      if (step === "add") heapAfterAdd = await heap(cdp);
    }
    return { steps, heapAfterLoad, heapAfterAdd, errors };
  } finally {
    await browser.close();
    await server.close();
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const arg = (flag, dflt) => { const i = process.argv.indexOf(flag); return i < 0 ? dflt : process.argv[i + 1]; };
  const result = await runTodomvc({ dir: arg("--dir"), npmTools: arg("--npm-tools"), todos: Number(arg("--todos", "1000")) });
  process.stdout.write(JSON.stringify(result) + "\n");
}
