import init, { compile, version } from "./pkg/light_cljs_playground.js";

const STORAGE_KEY = "light-cljs-playground";
const RUN_TIMEOUT_MS = 10_000;

const EXAMPLE = {
  entry: "demo.main",
  files: [
    {
      path: "demo/main.cljs",
      source: `(ns demo.main
  (:require [demo.math :as math]
            [demo.macros :refer [twice unless]]
            [clojure.string :as str]))

;; As in ClojureScript, println needs a print function: this one is console.log.
(enable-console-print!)

(defn describe [x]
  (str x " doubled is " (twice x)))

(println "squares:" (mapv math/square (range 1 6)))
(println (str/join ", " (map describe [1 2 3])))
(unless (empty? math/primes)
  (println "primes below 30:" math/primes))

;; The entry's last form is the program's value.
{:total (reduce + (map math/square (range 1 6)))
 :words (str/split "the quick brown fox" #" ")
 :sorted (into (sorted-map) {:b 2 :a 1 :c 3})}
`,
    },
    {
      path: "demo/math.cljs",
      source: `(ns demo.math)

(defn square [x] (* x x))

(defn prime? [n]
  (and (> n 1)
       (not-any? #(zero? (rem n %)) (range 2 (inc (js/Math.floor (js/Math.sqrt n)))))))

(def primes (vec (filter prime? (range 30))))
`,
    },
    {
      path: "demo/macros.cljs",
      source: `(ns demo.macros)

;; Macros are ordinary defmacros in .cljs files; the compiler runs them in
;; its embedded JavaScript engine while compiling.
(defmacro twice [x]
  \`(let [v# ~x] (+ v# v#)))

(defmacro unless [test & body]
  \`(if ~test nil (do ~@body)))
`,
    },
  ],
};

const el = (id) => document.getElementById(id);
const ui = {
  entry: el("entry"),
  optimize: el("optimize"),
  warnings: el("warnings"),
  pseudo: el("pseudo"),
  sizes: el("sizes"),
  run: el("run"),
  compile: el("compile"),
  stop: el("stop"),
  reset: el("reset"),
  status: el("status"),
  fileTabs: el("file-tabs"),
  consoleText: el("console-text"),
  diagnosticsText: el("diagnostics-text"),
};

let state = load() || structuredClone(EXAMPLE);
let active = 0;
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

function compileAndRun(andRun) {
  stopRun();
  ui.consoleText.innerHTML = "";
  ui.diagnosticsText.innerHTML = "";
  state.entry = ui.entry.value.trim();
  state.optimize = ui.optimize.value;
  state.warnings = ui.warnings.checked;
  state.pseudo = ui.pseudo.checked;
  state.sizes = ui.sizes.checked;
  save();
  setStatus("compiling…");
  // Let the status paint before the synchronous compile.
  setTimeout(() => {
    const result = compile(
      state.files.map((file) => file.path),
      state.files.map((file) => file.source),
      state.entry,
      state.optimize,
      state.warnings,
      state.pseudo,
      state.sizes,
      false,
    );
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
  }, 0);
}

function applyState() {
  ui.entry.value = state.entry || EXAMPLE.entry;
  ui.optimize.value = state.optimize || "prepare";
  ui.warnings.checked = !!state.warnings;
  ui.pseudo.checked = !!state.pseudo;
  ui.sizes.checked = !!state.sizes;
  selectFile(0);
}

ui.run.onclick = () => compileAndRun(true);
ui.compile.onclick = () => compileAndRun(false);
ui.stop.onclick = stopRun;
ui.reset.onclick = () => {
  if (!confirm("Replace all files with the example?")) return;
  state = structuredClone(EXAMPLE);
  save();
  applyState();
};
document.querySelectorAll("#results .tab").forEach((tab) => (tab.onclick = () => showView(tab.dataset.view)));
document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
    event.preventDefault();
    compileAndRun(!event.shiftKey);
  }
});

applyState();
ui.run.disabled = ui.compile.disabled = true;
init()
  .then(() => {
    ui.run.disabled = ui.compile.disabled = false;
    setStatus(`light-cljs playground ${version()} ready`);
  })
  .catch((error) => {
    setStatus(`failed to load the compiler: ${error}`, true);
  });
