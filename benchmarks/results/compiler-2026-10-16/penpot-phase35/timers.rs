use std::{cell::RefCell, collections::BTreeMap, time::{Duration, Instant}};
#[derive(Default)]
struct Stats {
    entries: BTreeMap<(String, &'static str), (usize, Duration, Duration)>,
    children: Vec<Duration>,
}
impl Drop for Stats {
    fn drop(&mut self) {
        assert!(self.children.is_empty());
        for ((name, stage), (count, inclusive, exclusive)) in &self.entries {
            eprintln!("PHASE35\t{name}\t{stage}\t{count}\t{:.6}\t{:.6}", inclusive.as_secs_f64(), exclusive.as_secs_f64());
        }
    }
}
thread_local! { static STATS: RefCell<Stats> = RefCell::new(Stats::default()); }
pub(crate) struct Timer(String, &'static str, Instant);
pub(crate) fn timer(name: &str, stage: &'static str) -> Timer {
    STATS.with(|s| s.borrow_mut().children.push(Duration::ZERO));
    Timer(name.to_owned(), stage, Instant::now())
}
impl Drop for Timer {
    fn drop(&mut self) {
        let duration = self.2.elapsed();
        STATS.with(|s| {
            let mut s = s.borrow_mut();
            let children = s.children.pop().unwrap();
            if let Some(parent) = s.children.last_mut() { *parent += duration; }
            let entry = s.entries.entry((self.0.clone(), self.1)).or_default();
            entry.0 += 1;
            entry.1 += duration;
            entry.2 += duration.saturating_sub(children);
        });
    }
}
pub(crate) fn measure<T>(name: &str, stage: &'static str, f: impl FnOnce() -> T) -> T {
    let _timer = timer(name, stage);
    f()
}
