//! Opt-in aggregate diagnostic intervals. Nested intervals overlap.
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::Instant,
};
static ENABLED: OnceLock<bool> = OnceLock::new();
static ROWS: Mutex<BTreeMap<String, (u64, u128)>> = Mutex::new(BTreeMap::new());
pub struct Scope(Option<(String, Instant)>);
pub fn scope(key: impl FnOnce() -> String) -> Scope {
    Scope(
        (*ENABLED.get_or_init(|| std::env::var_os("LIGHT_CLJS_MACRO_PROBE").is_some()))
            .then(|| (key(), Instant::now())),
    )
}
impl Drop for Scope {
    fn drop(&mut self) {
        if let Some((key, begun)) = &self.0 {
            let elapsed = begun.elapsed().as_nanos();
            let mut rows = ROWS.lock().unwrap();
            let row = rows.entry(key.clone()).or_default();
            row.0 += 1;
            row.1 += elapsed;
        }
    }
}
pub fn write() -> std::io::Result<()> {
    if let Some(path) = std::env::var_os("LIGHT_CLJS_MACRO_PROBE") {
        let mut text = String::from("# inclusive overlapping intervals: key\tcalls\tnanoseconds\n");
        for (key, (calls, nanos)) in ROWS.lock().unwrap().iter() {
            text.push_str(&format!("{key}\t{calls}\t{nanos}\n"));
        }
        std::fs::write(path, text)?;
    }
    Ok(())
}
