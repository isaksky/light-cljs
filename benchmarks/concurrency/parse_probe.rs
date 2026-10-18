use light_cljs::{Parser, SyntaxTree};
use std::{fs, hint::black_box, path::Path, sync::atomic::{AtomicUsize, Ordering}, time::Instant};

#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn collect(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() { collect(&path, files); }
        else if matches!(path.extension().and_then(|x| x.to_str()), Some("cljs" | "cljc")) { files.push(path); }
    }
}
fn parse(source: &str) -> SyntaxTree {
    let mut parser = Parser::new(source).retaining_discards();
    let mut roots = vec![];
    while let Some(root) = parser.read_form().unwrap() { roots.push(root); }
    SyntaxTree { nodes: parser.into_nodes(), roots }
}
fn run(sources: &[String], threads: usize, batch: usize) -> (f64, usize) {
    let next = AtomicUsize::new(0);
    let start = Instant::now();
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads).map(|_| {
            let next = &next;
            scope.spawn(move || {
                let mut trees = vec![];
                loop {
                    let first = next.fetch_add(batch, Ordering::Relaxed);
                    if first >= sources.len() { break; }
                    for source in &sources[first..(first + batch).min(sources.len())] {
                        trees.push(parse(source));
                    }
                }
                trees
            })
        }).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect::<Vec<_>>()
    });
    let millis = start.elapsed().as_secs_f64() * 1000.0;
    let nodes = results.iter().flatten().map(|t| t.nodes.len()).sum();
    black_box(&results);
    (millis, nodes)
}
fn main() {
    fn transferable<T: Send + Sync>() {}
    transferable::<SyntaxTree>();
    let mut files = vec![];
    for root in std::env::args().skip(1) { collect(Path::new(&root), &mut files); }
    files.sort(); files.dedup();
    let sources: Vec<_> = files.iter().map(|p| fs::read_to_string(p).unwrap()).collect();
    println!("files={},bytes={}", sources.len(), sources.iter().map(String::len).sum::<usize>());
    black_box(run(&sources, 1, 1));
    println!("sample,threads,batch,millis,nodes");
    let configs = [(1,1),(2,1),(4,1),(8,1),(1,4),(2,4),(4,4),(8,4)];
    for sample in 0..7 {
        for offset in 0..configs.len() {
            let (threads, batch) = configs[(offset + sample) % configs.len()];
            let (millis, nodes) = run(&sources, threads, batch);
            println!("{sample},{threads},{batch},{millis:.3},{nodes}");
        }
    }
}
