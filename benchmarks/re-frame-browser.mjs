// re-frame's example apps in headless Chromium, driven by Playwright. Run by
// `bb benchmarks/re-frame.bb browser`; usable alone:
//
//   node benchmarks/re-frame-browser.mjs --app todomvc --dir DIR --npm-tools DIR [--runs 3]
//
// DIR is a copy of the example's resources/public with js/client.js replaced
// by one compiler's build (`bb benchmarks/re-frame.bb bundle`). The script
// serves DIR over HTTP on a free local port, opens it in Chromium and runs the
// app's checks, each in a fresh browser context (empty localStorage) unless
// the check reloads the page itself. It prints one JSON object:
//   {app, dir, runs: [{checks: [{name, ok, error, ms}], errors: [...], snapshots}]}
// `errors` holds console errors and uncaught exceptions from every check of
// the run; `snapshots` holds, for flow, the page text after each step, which
// the caller compares across compilers.
//
// Other scripts import serve, launch, openApp and todomvc from here.
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".map": "application/json" };

/** Serve DIR on 127.0.0.1 and a free port: {url, close}. */
export async function serve(dir) {
  const root = path.resolve(dir);
  const server = http.createServer((req, res) => {
    const rel = decodeURIComponent(new URL(req.url, "http://x").pathname);
    const file = path.join(root, rel.endsWith("/") ? rel + "index.html" : rel);
    if (!file.startsWith(root) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
      res.writeHead(404).end("not found");
      return;
    }
    res.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" });
    fs.createReadStream(file).pipe(res);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return { url: `http://127.0.0.1:${server.address().port}/`, close: () => new Promise((r) => server.close(r)) };
}

/** Headless Chromium from playwright-core installed under NPM_TOOLS. */
export async function launch(npmTools) {
  const require = createRequire(path.join(path.resolve(npmTools), "package.json"));
  const { chromium } = require("playwright-core");
  return chromium.launch({ headless: true });
}

/** A new context and page on URL, recording console errors and uncaught
 *  exceptions into ERRORS. */
export async function openApp(browser, url, errors) {
  const context = await browser.newContext();
  const page = await context.newPage();
  page.on("console", (m) => { if (m.type() === "error") errors.push(`console: ${m.text().split("\n")[0]}`); });
  page.on("pageerror", (e) => errors.push(`exception: ${e.message} ${(e.stack ?? "").split("\n")[1]?.trim() ?? ""}`));
  page.setDefaultTimeout(5000);
  await page.goto(url);
  return { context, page };
}

function assert(ok, message) { if (!ok) throw new Error(message); }

// re-frame handles events in a later task and reagent renders on the next
// animation frame, so every check of the page after an action polls.
async function eventually(predicate, what, ms = 2000) {
  const deadline = Date.now() + ms;
  do {
    if (await predicate()) return;
    await new Promise((r) => setTimeout(r, 25));
  } while (Date.now() < deadline);
  throw new Error(what);
}

/** Click a controlled checkbox and wait until it shows STATE: Playwright's
 *  check() fails on it, since React holds the old state until the event's
 *  re-render. */
async function toggle(checkbox, state) {
  await checkbox.click();
  await eventually(async () => (await checkbox.isChecked()) === state, `checkbox did not become ${state}`);
}

const hasClass = async (locator, cls) => ((await locator.getAttribute("class")) ?? "").split(/\s+/).includes(cls);

async function expectTexts(locator, expected, what) {
  const deadline = Date.now() + 2000;
  let actual;
  do {
    actual = (await locator.allInnerTexts()).map((s) => s.trim());
    if (JSON.stringify(actual) === JSON.stringify(expected)) return;
    await new Promise((r) => setTimeout(r, 25));
  } while (Date.now() < deadline);
  throw new Error(`${what}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
}

async function expectText(locator, expected, what) {
  await expectTexts(locator, [expected], what);
}

// ---------------------------------------------------------------------------
// TodoMVC: the TodoMVC application specification's behaviours, against
// re-frame's markup (ids, not classes, for the singletons).

export const todomvc = {
  items: (page) => page.locator("#todo-list li"),
  labels: (page) => page.locator("#todo-list li label"),
  count: (page) => page.locator("#todo-count"),
  async add(page, ...titles) {
    for (const t of titles) {
      await page.locator("#new-todo").fill(t);
      await page.locator("#new-todo").press("Enter");
    }
  },
  async edit(page, index, text, key = "Enter") {
    await todomvc.labels(page).nth(index).dblclick();
    const edit = todomvc.items(page).nth(index).locator("input.edit");
    await edit.fill(text);
    if (key === "blur") await edit.evaluate((e) => e.blur());
    else await edit.press(key);
  },
};

const todomvcChecks = {
  async "renders the app"(page) {
    await expectText(page.locator("#header h1"), "todos", "heading");
    await eventually(async () => await page.locator("#new-todo").getAttribute("placeholder") === "What needs to be done?", "placeholder");
    await eventually(async () => await todomvc.items(page).count() === 0, "list is empty at start");
    await eventually(async () => await page.locator("#main").count() === 0, "main section hidden without todos");
    await eventually(async () => await page.locator("#new-todo").evaluate((e) => e === document.activeElement), "new-todo focused on load");
  },
  async "add todos"(page) {
    await todomvc.add(page, "buy milk", "  walk the dog  ");
    await expectTexts(todomvc.labels(page), ["buy milk", "walk the dog"], "added and trimmed");
    await eventually(async () => await page.locator("#new-todo").inputValue() === "", "input cleared after adding");
    await todomvc.add(page, "   ");
    await eventually(async () => await todomvc.items(page).count() === 2, "blank todo not added");
    await expectText(todomvc.count(page), "2 items left", "counter");
  },
  async "complete a todo"(page) {
    await todomvc.add(page, "a", "b");
    await toggle(todomvc.items(page).nth(0).locator(".toggle"), true);
    await expectText(todomvc.count(page), "1 item left", "counter after completing");
    await eventually(() => hasClass(todomvc.items(page).nth(0), "completed"), "li marked completed");
    await toggle(todomvc.items(page).nth(0).locator(".toggle"), false);
    await expectText(todomvc.count(page), "2 items left", "counter after un-completing");
    await eventually(async () => !(await hasClass(todomvc.items(page).nth(0), "completed")), "li unmarked");
  },
  async "toggle all"(page) {
    await todomvc.add(page, "a", "b", "c");
    await toggle(page.locator("#toggle-all"), true);
    await expectText(todomvc.count(page), "0 items left", "all completed");
    await eventually(async () => await page.locator("#todo-list li.completed").count() === 3, "every li completed");
    await toggle(page.locator("#toggle-all"), false);
    await expectText(todomvc.count(page), "3 items left", "none completed");
    await toggle(todomvc.items(page).nth(1).locator(".toggle"), true);
    await toggle(todomvc.items(page).nth(0).locator(".toggle"), true);
    await toggle(todomvc.items(page).nth(2).locator(".toggle"), true);
    await eventually(async () => await page.locator("#toggle-all").isChecked(), "toggle-all follows completing each todo");
  },
  async "edit a todo"(page) {
    await todomvc.add(page, "a", "b", "c");
    await todomvc.labels(page).nth(1).dblclick();
    await eventually(() => hasClass(todomvc.items(page).nth(1), "editing"), "li in editing mode");
    await eventually(async () => await todomvc.items(page).nth(1).locator("input.edit").inputValue() === "b", "edit field holds the title");
    await todomvc.items(page).nth(1).locator("input.edit").fill("  b2  ");
    await todomvc.items(page).nth(1).locator("input.edit").press("Enter");
    await expectTexts(todomvc.labels(page), ["a", "b2", "c"], "saved with Enter, trimmed");
    await todomvc.edit(page, 0, "a2", "blur");
    await expectTexts(todomvc.labels(page), ["a2", "b2", "c"], "saved on blur");
    await todomvc.edit(page, 2, "discarded", "Escape");
    await expectTexts(todomvc.labels(page), ["a2", "b2", "c"], "Escape cancels");
    await todomvc.edit(page, 1, "   ");
    await expectTexts(todomvc.labels(page), ["a2", "c"], "empty title deletes");
  },
  async "delete a todo"(page) {
    await todomvc.add(page, "a", "b");
    await todomvc.items(page).nth(0).hover();
    await todomvc.items(page).nth(0).locator(".destroy").click();
    await expectTexts(todomvc.labels(page), ["b"], "deleted");
    await expectText(todomvc.count(page), "1 item left", "counter");
  },
  async "filter routes"(page) {
    await todomvc.add(page, "a", "b", "c");
    await toggle(todomvc.items(page).nth(1).locator(".toggle"), true);
    await page.locator("#filters a", { hasText: "Active" }).click();
    await expectTexts(todomvc.labels(page), ["a", "c"], "Active");
    assert(page.url().endsWith("#/active"), `route #/active, at ${page.url()}`);
    await expectText(page.locator("#filters a.selected"), "Active", "Active link selected");
    await page.locator("#filters a", { hasText: "Completed" }).click();
    await expectTexts(todomvc.labels(page), ["b"], "Completed");
    await page.locator("#filters a", { hasText: "All" }).click();
    await expectTexts(todomvc.labels(page), ["a", "b", "c"], "All");
    await page.goBack();
    await expectTexts(todomvc.labels(page), ["b"], "back button returns to Completed");
    await page.goto(new URL("#/active", page.url()).href);
    await expectTexts(todomvc.labels(page), ["a", "c"], "route typed into the address bar");
  },
  async "clear completed"(page) {
    await todomvc.add(page, "a", "b", "c");
    await eventually(async () => await page.locator("#clear-completed").count() === 0, "button hidden with nothing completed");
    await toggle(todomvc.items(page).nth(0).locator(".toggle"), true);
    await toggle(todomvc.items(page).nth(2).locator(".toggle"), true);
    await expectText(page.locator("#clear-completed"), "Clear completed", "button shown");
    await page.locator("#clear-completed").click();
    await expectTexts(todomvc.labels(page), ["b"], "completed removed");
    await eventually(async () => await page.locator("#clear-completed").count() === 0, "button hidden again");
  },
  async "persistence"(page) {
    await todomvc.add(page, "a", "b");
    await toggle(todomvc.items(page).nth(1).locator(".toggle"), true);
    await expectText(todomvc.count(page), "1 item left", "before reload");
    await page.reload();
    await expectTexts(todomvc.labels(page), ["a", "b"], "todos after reload");
    await eventually(async () => await todomvc.items(page).nth(1).locator(".toggle").isChecked(), "completed state after reload");
    await expectText(todomvc.count(page), "1 item left", "counter after reload");
  },
};

// ---------------------------------------------------------------------------
// simple: a clock and a colour input.

const simpleChecks = {
  async "renders the clock"(page) {
    await expectText(page.locator("#app h1"), "The time is now:", "heading");
    const t = (await page.locator(".example-clock").innerText()).trim();
    assert(/^\d\d:\d\d:\d\d$/.test(t), `clock shows a time, got ${JSON.stringify(t)}`);
  },
  async "clock ticks"(page) {
    const before = await page.locator(".example-clock").innerText();
    await eventually(async () => await page.locator(".example-clock").innerText() !== before, "time changed within 2.5 s", 2500);
  },
  async "colour input recolours the clock"(page) {
    const input = page.locator(".color-input input");
    assert(await input.inputValue() === "orange", `initial colour, got ${await input.inputValue()}`);
    await input.fill("red");
    const color = () => page.locator(".example-clock").evaluate((e) => getComputedStyle(e).color);
    await eventually(async () => await color() === "rgb(255, 0, 0)", "clock turns red");
  },
};

// ---------------------------------------------------------------------------
// flow: buttons drive flows; the page text after each step is a snapshot the
// caller compares between the two compilers, plus a few absolute checks.

const flowSteps = [
  ["Add"], ["Add"], ["Add"], ["Add"], ["Delete"], ["Clear flow"], ["Add"], ["Register flow"],
  ["Delete"], ["Clear"], ["Add"],
];

const flowChecks = {
  async "renders and runs the flows"(page, run) {
    const text = async () => (await page.locator("#app").innerText()).trim();
    // The event's task, then two animation frames for reagent's render, until
    // the text stops changing.
    const settle = () => page.evaluate(() => new Promise((r) => setTimeout(() =>
      requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(r, 0))), 0)));
    const settledText = async () => {
      let before;
      for (let after = await text(); after !== before; before = after, await settle(), after = await text());
      return before;
    };
    await expectTexts(page.locator("#app button"), ["Add", "Delete", "Clear", "Clear flow", "Register flow"], "buttons");
    run.snapshots.push(["initial", await settledText()]);
    assert(run.snapshots[0][1].includes("No items."), "initial warning");
    for (const [button] of flowSteps) {
      await page.locator("#app button", { hasText: new RegExp(`^${button}$`) }).click();
      run.snapshots.push([button, await settledText()]);
    }
    assert(run.snapshots[1][1].includes("Item 1"), "Add shows Item 1");
    assert(run.snapshots[4][1].includes("only the first 3 items"), "too-many warning after four adds");
  },
};

export const checks = { todomvc: todomvcChecks, simple: simpleChecks, flow: flowChecks };

/** Run APP's checks RUNS times against DIR. */
export async function runChecks({ app, dir, npmTools, runs = 3 }) {
  const server = await serve(dir);
  const browser = await launch(npmTools);
  const result = { app, dir, runs: [] };
  try {
    for (let i = 0; i < runs; i++) {
      const run = { checks: [], errors: [], snapshots: [] };
      for (const [name, check] of Object.entries(checks[app])) {
        const errors = [];
        const start = performance.now();
        let opened;
        try {
          opened = await openApp(browser, server.url, errors);
          await check(opened.page, run);
          run.checks.push({ name, ok: true, ms: Math.round(performance.now() - start) });
        } catch (e) {
          run.checks.push({ name, ok: false, error: String(e.message ?? e).split("\n")[0], ms: Math.round(performance.now() - start) });
        } finally {
          await opened?.context.close();
        }
        run.errors.push(...errors.map((e) => `${name}: ${e}`));
      }
      result.runs.push(run);
    }
  } finally {
    await browser.close();
    await server.close();
  }
  return result;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const arg = (flag, dflt) => { const i = process.argv.indexOf(flag); return i < 0 ? dflt : process.argv[i + 1]; };
  const result = await runChecks({
    app: arg("--app"), dir: arg("--dir"), npmTools: arg("--npm-tools"), runs: Number(arg("--runs", "3")),
  });
  process.stdout.write(JSON.stringify(result) + "\n");
}
