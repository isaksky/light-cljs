import { createCompiler } from "./compiler.js";
import { EXAMPLES } from "./examples.js";

const STORAGE_KEY = "light-cljs-playground";
const RUN_TIMEOUT_MS = 10_000;

const el = (id) => document.getElementById(id);
const ui = {
  examples: el("examples"),
  entry: el("entry"),
  optimize: el("optimize"),
  warnings: el("warnings"),
  pseudo: el("pseudo"),
  sizes: el("sizes"),
  browserMacros: el("browser-macros"),
  run: el("run"),
  compile: el("compile"),
  stop: el("stop"),
  reset: el("reset"),
  status: el("status"),
  fileTabs: el("file-tabs"),
  consoleText: el("console-text"),
  diagnosticsText: el("diagnostics-text"),
};

let state = load() || fromExample(EXAMPLES[0]);
let active = 0;
let compiler = null;
let worker = null;
let runTimer = null;
let lastJs = "";

const editor = CodeMirror(el("editor"), {
  mode: "clojure",
  lineNumbers: true,
  matchBrackets: true,
  autoCloseBrackets: true,
  indentUnit: 2,
  tabSize: 2,
  extraKeys: {
    "Cmd-Enter": () => compileAndRun(true),
    "Ctrl-Enter": () => compileAndRun(true),
    "Shift-Cmd-Enter": () => compileAndRun(false),
    "Shift-Ctrl-Enter": () => compileAndRun(false),
  },
});
editor.on("change", () => {
  state.files[active].source = editor.getValue();
  save();
});

const jsEditor = CodeMirror(el("js-editor"), {
  mode: "javascript",
  lineNumbers: true,
  readOnly: true,
  lineWrapping: true,
});

function fromExample(example) {
  return { example: example.id, entry: example.entry, files: structuredClone(example.files) };
}

function currentExample() {
  return EXAMPLES.find((example) => example.id === state.example) || null;
}

// Whether the files differ from the example they came from.
function edited() {
  const example = currentExample();
  if (!example) return true;
  return (
    state.entry !== example.entry ||
    state.files.length !== example.files.length ||
    state.files.some((file, index) => file.path !== example.files[index].path || file.source !== example.files[index].source)
  );
}

function loadExample(example, { ask = true } = {}) {
  if (ask && edited() && !confirm(`Replace all files with "${example.title}"?`)) {
    renderExamples();
    return;
  }
  const options = { optimize: ui.optimize.value, warnings: ui.warnings.checked, pseudo: ui.pseudo.checked, sizes: ui.sizes.checked, browserMacros: ui.browserMacros.checked };
  state = Object.assign(fromExample(example), options);
  save();
  applyState();
}

function renderExamples() {
  ui.examples.innerHTML = "";
  for (const example of EXAMPLES) {
    const option = document.createElement("option");
    option.value = example.id;
    option.textContent = example.title;
    ui.examples.appendChild(option);
  }
  if (!currentExample()) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "(your files)";
    ui.examples.appendChild(option);
  }
  ui.examples.value = state.example && currentExample() ? state.example : "";
}

function load() {
  try {
    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY));
    if (stored && Array.isArray(stored.files) && stored.files.length) return stored;
  } catch (error) {
    // Unreadable or blocked storage: start from the example.
  }
  return null;
}

function save() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch (error) {
    // Storage is a convenience only.
  }
}

function setStatus(text, error = false) {
  ui.status.textContent = text;
  ui.status.classList.toggle("error", error);
}

function renderTabs() {
  ui.fileTabs.innerHTML = "";
  state.files.forEach((file, index) => {
    const tab = document.createElement("button");
    tab.className = "tab" + (index === active ? " active" : "");
    tab.textContent = file.path;
    tab.title = "Click to edit, double-click to rename";
    tab.onclick = () => selectFile(index);
    tab.ondblclick = () => renameFile(index);
    const close = document.createElement("span");
    close.className = "close";
    close.textContent = "×";
    close.title = "Remove file";
    close.onclick = (event) => {
      event.stopPropagation();
      removeFile(index);
    };
    tab.appendChild(close);
    ui.fileTabs.appendChild(tab);
  });
  const add = document.createElement("button");
  add.className = "tab add";
  add.textContent = "+ file";
  add.onclick = addFile;
  ui.fileTabs.appendChild(add);
}

function selectFile(index) {
  active = Math.max(0, Math.min(index, state.files.length - 1));
  renderTabs();
  editor.setValue(state.files[active].source);
  editor.clearHistory();
  editor.focus();
}

function addFile() {
  const path = prompt("File path (namespace dots become directories, hyphens underscores):", "demo/util.cljs");
  if (!path) return;
  if (state.files.some((file) => file.path === path)) return selectFile(state.files.findIndex((f) => f.path === path));
  const ns = path.replace(/\.clj[sc]$/, "").replace(/\//g, ".").replace(/_/g, "-");
  state.files.push({ path, source: `(ns ${ns})\n\n` });
  save();
  selectFile(state.files.length - 1);
}

function renameFile(index) {
  const path = prompt("New path:", state.files[index].path);
  if (!path || path === state.files[index].path) return;
  state.files[index].path = path;
  save();
  renderTabs();
}

function removeFile(index) {
  if (state.files.length === 1) return;
  if (!confirm(`Remove ${state.files[index].path}?`)) return;
  state.files.splice(index, 1);
  save();
  selectFile(Math.min(active, state.files.length - 1));
}

function showView(name) {
  document.querySelectorAll("#results .tab").forEach((tab) => tab.classList.toggle("active", tab.dataset.view === name));
  document.querySelectorAll("#results .view").forEach((view) => view.classList.toggle("active", view.id === name));
  if (name === "js") jsEditor.refresh();
}

function appendLine(target, text, kind = "") {
  const line = document.createElement("div");
  line.className = "line " + kind;
  line.textContent = text;
  target.appendChild(line);
  target.parentElement.scrollTop = target.parentElement.scrollHeight;
}

function jumpToError(result) {
  if (!result.error_file) return;
  const index = state.files.findIndex((file) => file.path === result.error_file);
  if (index < 0) return;
  selectFile(index);
  if (result.error_line > 0) {
    editor.setCursor({ line: result.error_line - 1, ch: Math.max(0, result.error_column - 1) });
  }
}

// Stops a pending compile and a running program.
function stop() {
  if (compiler && compiler.cancel()) setStatus("compile stopped");
  stopRun();
}

function stopRun() {
  if (worker) {
    worker.terminate();
    worker = null;
  }
  clearTimeout(runTimer);
  runTimer = null;
  ui.stop.disabled = true;
}

function run(js) {
  stopRun();
  worker = new Worker("worker.js");
  ui.stop.disabled = false;
  worker.onmessage = (event) => {
    const message = event.data;
    if (message.type === "console") {
      appendLine(ui.consoleText, message.text, message.level === "error" ? "error" : message.level === "warn" ? "warn" : "");
    } else if (message.type === "result") {
      appendLine(ui.consoleText, "=> " + message.text, "result");
      if (message.text === "[object Object]" && state.optimize === "advanced") {
        appendLine(
          ui.consoleText,
          "at --optimize advanced a program that never prints embeds no printer, so its collections have no toString; use str, pr-str or (enable-console-print!) to see the value",
          "meta",
        );
      }
      appendLine(ui.consoleText, `ran in ${message.millis.toFixed(1)} ms`, "meta");
      stopRun();
    } else if (message.type === "error") {
      appendLine(ui.consoleText, message.text, "error");
      stopRun();
    }
  };
  worker.onerror = (event) => {
    appendLine(ui.consoleText, event.message || String(event), "error");
    stopRun();
  };
  runTimer = setTimeout(() => {
    appendLine(ui.consoleText, `stopped after ${RUN_TIMEOUT_MS / 1000} s`, "error");
    stopRun();
  }, RUN_TIMEOUT_MS);
  worker.postMessage({ js });
}

// The compile runs in compiler-worker.js, so the editor stays responsive
// meanwhile; Stop cancels it, and only the newest request's result is shown.
async function compileAndRun(andRun) {
  if (!compiler) return;
  stopRun();
  ui.consoleText.innerHTML = "";
  ui.diagnosticsText.innerHTML = "";
  state.entry = ui.entry.value.trim();
  state.optimize = ui.optimize.value;
  state.warnings = ui.warnings.checked;
  state.pseudo = ui.pseudo.checked;
  state.sizes = ui.sizes.checked;
  state.browserMacros = ui.browserMacros.checked;
  save();
  setStatus("compiling…");
  ui.stop.disabled = false;
  const outcome = await compiler.compile({
    paths: state.files.map((file) => file.path),
    sources: state.files.map((file) => file.source),
    entry: state.entry,
    optimize: state.optimize,
    warnings: state.warnings,
    pseudo: state.pseudo,
    sizes: state.sizes,
    macros: state.browserMacros ? "browser" : "embedded",
  });
  // A newer compile owns the views, and stop reported a cancelled one.
  if (outcome.status === "stale" || outcome.status === "cancelled") return;
  ui.stop.disabled = true;
  if (outcome.status === "failed") {
    appendLine(ui.diagnosticsText, outcome.error, "error");
    appendLine(ui.consoleText, outcome.error, "error");
    setStatus(outcome.error, true);
    showView("console");
    return;
  }
  const result = outcome.result;
  if (result.warnings) {
    for (const line of result.warnings.split("\n")) appendLine(ui.diagnosticsText, line, "warn");
  }
  if (result.error != null) {
    appendLine(ui.diagnosticsText, result.error, "error");
    appendLine(ui.consoleText, result.error, "error");
    setStatus(`compile failed in ${result.millis.toFixed(0)} ms`, true);
    showView("console");
    jumpToError(result);
    return;
  }
  lastJs = result.js;
  jsEditor.setValue(lastJs);
  if (result.size_report) appendLine(ui.diagnosticsText, result.size_report, "");
  if (!ui.diagnosticsText.childElementCount) appendLine(ui.diagnosticsText, "no diagnostics", "meta");
  const bytes = new TextEncoder().encode(lastJs).length;
  setStatus(`compiled in ${result.millis.toFixed(0)} ms; ${bytes.toLocaleString()} bytes of JavaScript at --optimize ${state.optimize}`);
  if (andRun) {
    showView("console");
    run(lastJs);
  } else {
    showView("js");
  }
}

function applyState() {
  ui.entry.value = state.entry || "";
  renderExamples();
  ui.optimize.value = state.optimize || "prepare";
  ui.warnings.checked = !!state.warnings;
  ui.pseudo.checked = !!state.pseudo;
  ui.sizes.checked = !!state.sizes;
  // On unless turned off (light-cljs-356); state saved before 307 has no field.
  ui.browserMacros.checked = state.browserMacros ?? true;
  selectFile(0);
}

ui.run.onclick = () => compileAndRun(true);
ui.compile.onclick = () => compileAndRun(false);
ui.stop.onclick = stop;
ui.examples.onchange = () => {
  const example = EXAMPLES.find((candidate) => candidate.id === ui.examples.value);
  if (example) loadExample(example);
};
ui.reset.onclick = () => {
  const example = currentExample() || EXAMPLES[0];
  loadExample(example);
};
document.querySelectorAll("#results .tab").forEach((tab) => (tab.onclick = () => showView(tab.dataset.view)));
document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
    event.preventDefault();
    compileAndRun(!event.shiftKey);
  }
});

// Compiles the wasm module once, here, so a worker that replaces a stopped one
// only instantiates it.
async function loadCompiler() {
  const url = new URL("./pkg/light_cljs_playground_bg.wasm", import.meta.url);
  let module;
  try {
    module = await WebAssembly.compileStreaming(fetch(url));
  } catch (error) {
    // A server that does not send application/wasm.
    module = await WebAssembly.compile(await (await fetch(url)).arrayBuffer());
  }
  const script = new URL("./compiler-worker.js", import.meta.url);
  return createCompiler({ spawn: () => new Worker(script, { type: "module" }), module });
}

applyState();
ui.run.disabled = ui.compile.disabled = true;
loadCompiler()
  .then(async (loaded) => {
    const version = await loaded.ready;
    compiler = loaded;
    ui.run.disabled = ui.compile.disabled = false;
    setStatus(`light-cljs playground ${version} ready`);
  })
  .catch((error) => {
    setStatus(`failed to load the compiler: ${error.message || error}`, true);
  });
