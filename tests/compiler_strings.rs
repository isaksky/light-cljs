//! Compiler-owned clojure.string contracts in QuickJS, Node, and macro execution.
use light_cljs::{
    compile_namespace,
    evaluator::{Evaluator, EvaluatorOptions},
};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Sources(PathBuf);
impl Sources {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "light-cljs-strings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) {
        fs::write(self.0.join(name), source).unwrap();
    }
    fn compile(&self) -> String {
        compile_namespace("main", std::slice::from_ref(&self.0)).unwrap()
    }
}
impl Drop for Sources {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn check(name: &str, javascript: &str) {
    let expression = format!("JSON.stringify({javascript})");
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    let value = evaluator.evaluate_js(&expression).unwrap();
    assert_eq!(evaluator.string(value).unwrap(), "true", "QuickJS {name}");
    let mut child = Command::new("node")
        .args(["--max-old-space-size=96", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("console.log({expression});").as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "Node {name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "true\n",
        "Node {name}"
    );
}

#[test]
fn public_string_contracts_execute_in_quickjs_and_node() {
    let sources = Sources::new();
    for (name, body) in [
        (
            "join-and-callable-values",
            r#"(and
          (= "" (s/join nil)) (= "" (s/join [])) (= "12" (s/join [1 2]))
          (= "1,2,3" (s/join \, [1 2 3])) (= "x" (s/join "!" ["x"]))
          (= "a::false::[:k 1]" (s/join ":" ["a" nil false nil [:k 1]]))
          (= "0123" (s/join (map str (range 4))))
          (= "a-b" (apply s/join ["-" '("a" "b")]))
          (= "a b" ((get (ns-publics 'clojure.string) 'join) " " ["a" "b"])))"#,
        ),
        (
            "unicode-case-and-reverse",
            r#"(and
          (= "" (s/reverse "")) (= "tab" (s/reverse "bat"))
          (= "c\uD834\uDD1Ea" (s/reverse "a\uD834\uDD1Ec"))
          (= "b\uD800a" (s/reverse "a\uD800b"))
          (= "STRASSE" (s/upper-case "Straße")) (= "foo" (s/lower-case "FOO"))
          (= "Foobar" (s/capitalize "FOOBAR")) (= "" (s/capitalize ""))
          (= "SSeta" (s/capitalize "ßETA"))
          (= "i\u0307" (s/lower-case "\u0130")))"#,
        ),
        (
            "literal-and-regex-replacements",
            r#"(and
          (= "faabar" (s/replace "foobar" \o \a))
          (= "barbar)foo" (s/replace "foo(bar)foo" "foo(" "bar"))
          (= "$&a$&b$&" (s/replace "xaxbx" "x" "$&"))
          (= "-a-b-" (s/replace "ab" "" "-"))
          (= "-ab" (s/replace-first "ab" "" "-"))
          (= "aXXa" (s/replace "abba" #"b" "X"))
          (= "faabarfaa" (s/replace "FOObarfoo" #"(?i)foo" "faa"))
          (= "aaa\nccc" (s/replace "aaa\nbbb" #"(?m)^bbb" "ccc"))
          (= "x" (s/replace "a\nb" #"(?s)a.b" "x"))
          (= "21-43" (s/replace "12-34" #"(\d)(\d)" "$2$1"))
          (= "barbarfoo" (s/replace-first "foobarfoo" "foo" "bar"))
          (= "axx" (s/replace-first "xxx" #"(?g)x" "a")))"#,
        ),
        (
            "regex-and-escape-callbacks",
            r#"(let [seen (atom [])]
          (and
            (= "FOObarFOO" (s/replace "foobarfoo" #"foo" s/upper-case))
            (= "FOO-ObarFOO-O" (s/replace "foobarfoo" #"f(o)o"
                 (fn [[whole group]] (s/upper-case (str whole "-" group)))))
            (= "foo:o,foo:o" (s/replace "foo,foo" #"(?<v>o)o"
                 (fn [[whole group]] (str whole ":" group))))
            (= "FOO-Obarfoo" (s/replace-first "foobarfoo" #"f(o)o"
                 (fn [[whole group]] (s/upper-case (str whole "-" group)))))
            (= "&lt;foo&amp;bar&gt;" (s/escape "<foo&bar>" {\& "&amp;" \< "&lt;" \> "&gt;"}))
            (= "false0b" (s/escape "axb" {\a false \x 0}))
            (= "\uD83D\uDE00" (s/escape "\uD83D\uDE00" (fn [c] (swap! seen conj c) nil)))
            (= ["\uD83D" "\uDE00"] @seen)))"#,
        ),
        (
            "split-limits-and-capture-groups",
            r#"(and
          (= ["a" "b"] (s/split "a-b" #"-"))
          (= ["a-b-c"] (s/split "a-b-c" #"-" 1))
          (= ["a" "b-c"] (s/split "a-b-c" #"-" 2))
          (= ["a" "b" "c"] (s/split "a-b-c" #"-" 4))
          (= ["a" "b" ""] (s/split "a-b-" #"-" -1))
          (= ["a" "b"] (s/split "a-b-" #"-" 0))
          (= [] (s/split "ab" #"ab")) (= [""] (s/split "" #"-"))
          (= ["a" "-" "b" "-" "c"] (s/split "a-b-c" #"(-)" -1))
          (= ["a" "b-c"] (s/split "a-b-c" #"(-)" 2))
          (= ["a" "-" "b"] (s/split "a-b" #"(-)?" -1))
          (= ["a" "b" ""] (s/split "a-b-" #"-" 3))
          (vector? (s/split "abc" #"-")))"#,
        ),
        (
            "empty-regex-and-lines",
            r#"(and
          (= ["" "a" "b" "c" ""] (s/split "abc" (re-pattern "") -1))
          (= ["" "a" "b" "c"] (s/split "abc" (re-pattern "")))
          (= ["abc"] (s/split "abc" (re-pattern "") 1))
          (= ["" "abc"] (s/split "abc" (re-pattern "") 2))
          (= ["" "a" "bc"] (s/split "abc" (re-pattern "") 3))
          (= [] (s/split "" (re-pattern "")))
          (= ["" "\uD83D" "\uDE00"] (s/split "\uD83D\uDE00" (re-pattern "")))
          (= ["one" "two" "three"] (s/split-lines "one\ntwo\r\nthree\r\n"))
          (= ["a\rb"] (s/split-lines "a\rb"))
          (= [""] (s/split-lines "")) (= [] (s/split-lines "\n\n")))"#,
        ),
        (
            "whitespace-and-newlines",
            r#"(and
          (= "foo" (s/trim "  foo  \r\n")) (= "foo " (s/triml " foo "))
          (= " foo" (s/trimr " foo ")) (= "" (s/trim "\u00a0\ufeff"))
          (= "\u001c" (s/trim "\u001c"))
          (s/blank? nil) (s/blank? "") (s/blank? " \t\r\n\u00a0\ufeff")
          (not (s/blank? "\u001c")) (not (s/blank? "  foo  "))
          (= "foo" (s/trim-newline "foo\r\n\r\r\n"))
          (= "foo\r " (s/trim-newline "foo\r "))
          (= "" (s/trim-newline "")))"#,
        ),
        (
            "arities-types-and-regex-state",
            r#"(let [re (re-pattern "(?g)o")]
          (set! (.-lastIndex re) 2)
          (and
            (= "faa" (s/replace "foo" re "a")) (= 2 (.-lastIndex re))
            (= "foo" (s/trim " foo " :ignored-extra-argument))
            (every? true? (map (fn [f] (try (apply f []) false (catch :default e
              (s/includes? (.-message e) "Invalid arity"))))
              [s/join s/split s/index-of s/last-index-of]))
            (try (s/replace "x" 1 "y") false (catch :default e true))
            (try (s/replace "x" "x" nil) false (catch :default e true))
            (try (s/upper-case nil) false (catch :default e true))))"#,
        ),
        (
            "utf16-indexes-and-substrings",
            r#"(and
          (= 2 (s/index-of "tacos" \c)) (= 1 (s/index-of "tacos" "ac"))
          (= 3 (s/index-of "tacos" \o 2)) (= 0 (s/index-of "abc" \a -100))
          (nil? (s/index-of "abc" "z")) (nil? (s/index-of "abc" "b" 100))
          (= 3 (s/index-of "abc" "" 100))
          (= 4 (s/last-index-of "banana" \n)) (= 3 (s/last-index-of "banana" "an"))
          (= 2 (s/last-index-of "banana" \n 3))
          (= 0 (s/last-index-of "abc" \a -100))
          (= 3 (s/index-of "a\uD83D\uDE00b" "b"))
          (= 2 (s/index-of "a\uD83D\uDE00b" "\uDE00"))
          (s/starts-with? "clojure west" "clojure") (s/starts-with? "abc" "")
          (not (s/starts-with? "conj" "clojure"))
          (s/ends-with? "Clojure West" "West") (s/ends-with? "abc" "")
          (not (s/ends-with? "x" "xyz"))
          (s/includes? "Clojure Applied Book" "Applied")
          (not (s/includes? "Clojure Applied Book" "Living")))"#,
        ),
    ] {
        sources.write(
            "main.cljs",
            &format!("(ns main (:require [clojure.string :as s])) {body}"),
        );
        check(name, &sources.compile());
    }
}

#[test]
fn string_helpers_execute_in_compiled_macros_and_retain_namespace_vars() {
    let sources = Sources::new();
    sources.write(
        "macros.cljs",
        r#"(ns macros (:require [clojure.string :as s]))
      (defmacro text [] (list 'quote [(s/join ":" ["a" "b"])
         (s/replace "foobarfoo" #"f(o)o" (fn [[m g]] (str m "-" g)))
         (s/split "a-b-c" #"-" 2) (s/reverse "a\uD834\uDD1Ec")]))"#,
    );
    sources.write(
        "main.cljs",
        r#"(ns main (:require [clojure.string :as s] [macros :refer [text]]))
      (and (= ["a:b" "foo-obarfoo-o" ["a" "b-c"] "c\uD834\uDD1Ea"] (text))
           (= "a:b" (s/join ":" ["a" "b"]))
           (= 20 (count (ns-publics 'clojure.string)))
           (not (contains? (ns-publics 'clojure.string) 'replace*)))"#,
    );
    check("compiled macros and public Vars", &sources.compile());
}
