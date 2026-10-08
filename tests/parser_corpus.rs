use std::{fs, path::Path};

use light_cljs::parse;

fn visit(path: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            if entry.file_name() != ".git" {
                visit(&entry.path(), files);
            }
        } else if matches!(
            entry.path().extension().and_then(|s| s.to_str()),
            Some("clj" | "cljs" | "cljc")
        ) {
            files.push(entry.path());
        }
    }
}

#[test]
#[ignore = "requires the gitignored research/clojurescript checkout"]
fn parse_clojurescript_checkout() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("research/clojurescript");
    let mut files = Vec::new();
    visit(&root, &mut files);
    files.sort();
    let (mut bytes, mut nodes, mut roots) = (0, 0, 0);
    let mut errors = Vec::new();
    for file in &files {
        let source = fs::read_to_string(file).unwrap();
        bytes += source.len();
        match parse(&source) {
            Ok(tree) => {
                nodes += tree.nodes.len();
                roots += tree.roots.len();
                for node in tree.nodes {
                    assert!(source.get(node.span).is_some(), "{}", file.display());
                }
            }
            Err(error) => errors.push(format!("{}: {error:?}", file.display())),
        }
    }
    assert!(files.len() > 100);
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    eprintln!(
        "Parsed {} files, {bytes} bytes, {nodes} nodes, {roots} top-level forms",
        files.len()
    );
}
