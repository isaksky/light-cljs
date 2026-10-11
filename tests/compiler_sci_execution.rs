//! Source-specific audit driver. The Python catalog validates identities and
//! compares these raw observations to independently retained SCI expectations.
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};

fn unhex(text: &str) -> String {
    assert_eq!(text.len() % 2, 0);
    String::from_utf8(
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect(),
    )
    .unwrap()
}

#[test]
#[ignore = "explicit source-pinned SCI Node/QuickJS audit; Python checks every observation"]
fn retained_sci_sources_execute_in_node_and_quickjs() {
    let input = std::env::var("LIGHT_CLJS_SCI_EXECUTION_INPUT").expect("catalog TSV input");
    let output = std::env::var("LIGHT_CLJS_SCI_EXECUTION_OUTPUT").expect("raw output TSV");
    let rows = fs::read_to_string(input).unwrap();
    let mut observations = fs::File::create(output).unwrap();
    let mut count = 0;
    for line in rows.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 2);
        let id = fields[0];
        assert!(id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'));
        let source = unhex(fields[1]);
        let wrapped = format!("(fn* [observe] {source})");
        let mut host = light_cljs::MacroHost::new();
        let compiled = light_cljs::parse(&wrapped)
            .map_err(|error| format!("{error:?}"))
            .and_then(|syntax| {
                light_cljs::analyze_with_macros(&wrapped, &syntax, &mut host)
                    .map_err(|error| format!("{error:?}"))
            });
        let unit = match compiled {
            Ok(program) => light_cljs::emitter::emit_js_unit(&program),
            Err(error) => {
                writeln!(
                    observations,
                    "{id}\trejected\t{}",
                    format!("{error:?}").replace(['\n', '\t'], " ")
                )
                .unwrap();
                count += 1;
                continue;
            }
        };
        let script = format!(
            "(() => {{ const rt = {}; const fn = ({unit})(rt); return ({})(rt)('{id}', fn); }})()",
            light_cljs::evaluator::RUNTIME_SOURCE,
            include_str!("support/sci_execution_observation.js")
        );
        let mut node = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        node.stdin
            .take()
            .unwrap()
            .write_all(format!("console.log({script});").as_bytes())
            .unwrap();
        let result = node.wait_with_output().unwrap();
        let node = if result.status.success() {
            String::from_utf8(result.stdout)
                .unwrap()
                .trim_end()
                .to_string()
        } else {
            format!(
                "HARNESS:{}",
                String::from_utf8_lossy(&result.stderr).replace(['\n', '\t'], " ")
            )
        };
        let runtime = rquickjs::Runtime::new().unwrap();
        runtime.set_memory_limit(128 * 1024 * 1024);
        runtime.set_max_stack_size(4 * 1024 * 1024);
        let context = rquickjs::Context::full(&runtime).unwrap();
        let quickjs = context.with(|ctx| match ctx.eval::<String, _>(script.as_bytes()) {
            Ok(value) => value,
            Err(error) => {
                format!("HARNESS:{error:?}: {:?}", ctx.catch()).replace(['\n', '\t'], " ")
            }
        });
        writeln!(observations, "{id}\texecuted\t{node}\t{quickjs}").unwrap();
        count += 1;
        if count % 100 == 0 {
            eprintln!("SCI Node/QuickJS raw observations: {count}");
        }
    }
    assert!(count > 0, "zero source executions");
}
