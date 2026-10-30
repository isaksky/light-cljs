//! Canonical JSON for comparing engine observations: the text `JSON.stringify`
//! prints for the parsed value with every object's keys sorted. QuickJS
//! computes it in builds that embed QuickJS; Node computes it in the Pants
//! build, which embeds none, so the Node half of a comparison still runs there.

/// What Node prints running `program` from its standard input; the run must succeed.
pub fn node_stdout(program: &str) -> String {
    use std::io::Write;
    let mut child =
        std::process::Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
            .arg("-")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(program.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

const SORT: &str = "(function sort(v) { if (Array.isArray(v)) return v.map(sort); if (v && typeof v === 'object') { const out = {}; Object.keys(v).sort().forEach(k => out[k] = sort(v[k])); return out; } return v; })";

/// The rows of strings the JavaScript expression `extract` gives, an array of
/// arrays of strings (a fixture catalog's cases), evaluated as [`canonical`] is.
pub fn rows(extract: &str) -> Vec<Vec<String>> {
    #[cfg(feature = "quickjs")]
    {
        let runtime = rquickjs::Runtime::new().unwrap();
        let context = rquickjs::Context::full(&runtime).unwrap();
        context.with(|ctx| ctx.eval::<Vec<Vec<String>>, _>(extract.as_bytes()).unwrap())
    }
    #[cfg(not(feature = "quickjs"))]
    {
        // encodeURIComponent never yields `,` or a line break, which separate cells and rows.
        let text = node(&format!(
            "({extract}).map(r => r.map(encodeURIComponent).join(',')).join('\\n')"
        ));
        text.lines()
            .map(|row| row.split(',').map(decode).collect())
            .collect()
    }
}

#[cfg(not(feature = "quickjs"))]
fn decode(cell: &str) -> String {
    let bytes = cell.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.push(u8::from_str_radix(&cell[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

/// What Node prints for the JavaScript string expression `expression`. It goes
/// through standard input: an expression embedding a fixture catalog is larger
/// than the operating system lets one command-line argument be.
pub fn node(expression: &str) -> String {
    node_stdout(&format!("process.stdout.write({expression})"))
}

/// The canonical text of the JSON text `value`.
pub fn canonical(value: &str) -> String {
    let script = format!("JSON.stringify({SORT}(JSON.parse({value:?})))");
    #[cfg(feature = "quickjs")]
    {
        let runtime = rquickjs::Runtime::new().unwrap();
        let context = rquickjs::Context::full(&runtime).unwrap();
        context.with(|ctx| ctx.eval::<String, _>(script.as_bytes()).unwrap())
    }
    #[cfg(not(feature = "quickjs"))]
    {
        node(&script)
    }
}

/// One command-line argument may hold 128 KiB on Linux, and all of them 1 MiB
/// on macOS; fixture catalogs (over 300 KB) and this 2 MiB value exceed those.
#[test]
fn expressions_larger_than_a_command_line_argument_evaluate() {
    let text = "x".repeat(2 << 20);
    assert_eq!(
        canonical(&format!("{{\"b\":1,\"a\":{text:?}}}")),
        format!("{{\"a\":{text:?},\"b\":1}}")
    );
    assert_eq!(
        rows(&format!("[[{text:?}, \"y\"]]")),
        vec![vec![text, "y".to_string()]]
    );
}
