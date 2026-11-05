//! Process-wide counts of the compile-time engines the process created
//! (light-cljs-778): requests for the V8 platform, V8 isolates, QuickJS
//! runtimes and Pants runtimes. The Goose compiler path creates none of
//! them; its report and `--stats` print the counts, which the CLI's one
//! build per process makes exact, and tests compare a census taken before
//! a build with one taken after it while nothing else runs.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static V8_PLATFORM: AtomicU64 = AtomicU64::new(0);
static V8_ISOLATES: AtomicU64 = AtomicU64::new(0);
static QUICKJS_RUNTIMES: AtomicU64 = AtomicU64::new(0);
static PANTS_RUNTIMES: AtomicU64 = AtomicU64::new(0);
thread_local! {
    /// The same counts for the engines made on this thread alone, which a
    /// test on one thread compares without the other tests' engines.
    static THREAD: std::cell::Cell<EngineCensus> = const {
        std::cell::Cell::new(EngineCensus {
            v8_platform: 0,
            v8_isolates: 0,
            quickjs_runtimes: 0,
            pants_runtimes: 0,
        })
    };
}
fn on_thread(count: impl FnOnce(&mut EngineCensus)) {
    THREAD.with(|census| {
        let mut now = census.get();
        count(&mut now);
        census.set(now);
    });
}

/// The engines created so far, or between two censuses ([`Self::since`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EngineCensus {
    /// Requests for the V8 platform: its initialization, which the first
    /// one performs, and each isolate's.
    pub v8_platform: u64,
    pub v8_isolates: u64,
    pub quickjs_runtimes: u64,
    pub pants_runtimes: u64,
}
impl EngineCensus {
    /// The engines created up to now.
    pub fn now() -> Self {
        Self {
            v8_platform: V8_PLATFORM.load(Relaxed),
            v8_isolates: V8_ISOLATES.load(Relaxed),
            quickjs_runtimes: QUICKJS_RUNTIMES.load(Relaxed),
            pants_runtimes: PANTS_RUNTIMES.load(Relaxed),
        }
    }
    /// The engines created up to now on this thread.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn on_this_thread() -> Self {
        THREAD.with(std::cell::Cell::get)
    }
    /// The engines created since `earlier`.
    pub fn since(self, earlier: Self) -> Self {
        Self {
            v8_platform: self.v8_platform - earlier.v8_platform,
            v8_isolates: self.v8_isolates - earlier.v8_isolates,
            quickjs_runtimes: self.quickjs_runtimes - earlier.quickjs_runtimes,
            pants_runtimes: self.pants_runtimes - earlier.pants_runtimes,
        }
    }
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
    /// The census as `key value` pairs, in the order of [`Self::line`].
    pub fn counts(&self) -> [(&'static str, u64); 4] {
        [
            ("v8-platform", self.v8_platform),
            ("v8-isolates", self.v8_isolates),
            ("quickjs-runtimes", self.quickjs_runtimes),
            ("pants-runtimes", self.pants_runtimes),
        ]
    }
    /// The Goose report's line: `ENGINES v8-platform=0 v8-isolates=0 ...`.
    pub fn line(&self) -> String {
        let mut line = String::from("ENGINES");
        for (key, count) in self.counts() {
            line.push_str(&format!(" {key}={count}"));
        }
        line
    }
}
#[cfg_attr(not(light_v8), allow(dead_code))]
pub(crate) fn v8_platform() {
    V8_PLATFORM.fetch_add(1, Relaxed);
    on_thread(|census| census.v8_platform += 1);
}
#[cfg_attr(not(light_v8), allow(dead_code))]
pub(crate) fn v8_isolate() {
    V8_ISOLATES.fetch_add(1, Relaxed);
    on_thread(|census| census.v8_isolates += 1);
}
#[cfg_attr(not(light_quickjs), allow(dead_code))]
pub(crate) fn quickjs_runtime() {
    QUICKJS_RUNTIMES.fetch_add(1, Relaxed);
    on_thread(|census| census.quickjs_runtimes += 1);
}
#[cfg_attr(not(feature = "bbp-macros"), allow(dead_code))]
pub(crate) fn pants_runtime() {
    PANTS_RUNTIMES.fetch_add(1, Relaxed);
    on_thread(|census| census.pants_runtimes += 1);
}

#[cfg(test)]
mod tests {
    use super::EngineCensus;
    #[test]
    fn a_census_counts_each_engine_kind_since_an_earlier_one() {
        let earlier = EngineCensus {
            v8_platform: 1,
            v8_isolates: 1,
            quickjs_runtimes: 0,
            pants_runtimes: 2,
        };
        let later = EngineCensus {
            v8_platform: 2,
            v8_isolates: 3,
            quickjs_runtimes: 3,
            pants_runtimes: 6,
        };
        let created = later.since(earlier);
        assert_eq!(
            created.line(),
            "ENGINES v8-platform=1 v8-isolates=2 quickjs-runtimes=3 pants-runtimes=4"
        );
        assert!(!created.is_empty());
        assert!(later.since(later).is_empty());
        assert_eq!(
            EngineCensus::default().line(),
            "ENGINES v8-platform=0 v8-isolates=0 quickjs-runtimes=0 pants-runtimes=0"
        );
    }
    /// Each engine constructor of the build counts itself, through its
    /// ordinary and its build path: the census the Goose path's zero
    /// assertions read misses no engine. Other tests only add to the
    /// process-wide counts, so each delta is at least the engines made here.
    #[test]
    fn real_engine_constructors_increment_the_census() {
        use crate::evaluator::{BuildResources, EvaluatorOptions};
        let options = EvaluatorOptions::default();
        let resources = BuildResources::new(&options, 2).unwrap();
        // The engines `make` creates: process-wide (at least), and on this
        // thread (exactly, as nothing else runs on it).
        fn made<T>(make: impl FnOnce() -> T) -> (T, EngineCensus, EngineCensus) {
            let (before, local) = (EngineCensus::now(), EngineCensus::on_this_thread());
            let value = make();
            (
                value,
                EngineCensus::now().since(before),
                EngineCensus::on_this_thread().since(local),
            )
        }
        #[cfg(light_v8)]
        {
            let (engines, created, here) = made(|| {
                (
                    crate::v8_evaluator::V8Evaluator::new(options.clone()).unwrap(),
                    crate::v8_evaluator::V8Evaluator::new_for_build(
                        options.clone(),
                        &resources,
                        false,
                    )
                    .unwrap(),
                )
            });
            assert!(created.v8_isolates >= 2, "{created:?}");
            assert!(created.v8_platform >= 2, "{created:?}");
            assert_eq!((here.v8_isolates, here.v8_platform), (2, 2), "{here:?}");
            drop(engines);
            let ((), created, here) = made(crate::v8_evaluator::initialize_platform);
            assert!(created.v8_platform >= 1);
            assert_eq!(here.v8_platform, 1, "{here:?}");
        }
        #[cfg(light_quickjs)]
        {
            let (engines, created, here) = made(|| {
                (
                    crate::evaluator::Evaluator::new(options.clone()).unwrap(),
                    crate::evaluator::Evaluator::interactive(options.clone()).unwrap(),
                )
            });
            assert!(created.quickjs_runtimes >= 2, "{created:?}");
            assert_eq!(here.quickjs_runtimes, 2, "{here:?}");
            drop(engines);
            #[cfg(not(light_v8))]
            {
                let (pooled, created, here) = made(|| {
                    crate::evaluator::Evaluator::new_for_build(options.clone(), &resources, false)
                        .unwrap()
                });
                assert!(created.quickjs_runtimes >= 1);
                assert_eq!(here.quickjs_runtimes, 1, "{here:?}");
                drop(pooled);
            }
        }
        #[cfg(feature = "bbp-macros")]
        {
            let (engines, created, here) = made(|| {
                (
                    crate::bbp_evaluator::BbpEvaluator::new(options.clone()).unwrap(),
                    crate::bbp_evaluator::BbpRuntime::new(&options).unwrap(),
                )
            });
            assert!(created.pants_runtimes >= 2, "{created:?}");
            assert!(here.pants_runtimes >= 2, "{here:?}");
            drop(engines);
        }
        drop(resources);
    }
    /// An engine made on another thread moves the process-wide census and
    /// that thread's, never this thread's.
    #[cfg(any(light_v8, light_quickjs, feature = "bbp-macros"))]
    #[test]
    fn the_thread_census_counts_only_this_threads_engines() {
        let local = EngineCensus::on_this_thread();
        let there = std::thread::spawn(|| {
            let before = EngineCensus::on_this_thread();
            let options = crate::evaluator::EvaluatorOptions::default();
            #[cfg(light_v8)]
            drop(crate::v8_evaluator::V8Evaluator::new(options).unwrap());
            #[cfg(all(light_quickjs, not(light_v8)))]
            drop(crate::evaluator::Evaluator::new(options).unwrap());
            #[cfg(not(any(light_v8, light_quickjs)))]
            drop(crate::bbp_evaluator::BbpRuntime::new(&options).unwrap());
            EngineCensus::on_this_thread().since(before)
        })
        .join()
        .unwrap();
        assert!(!there.is_empty(), "{there:?}");
        assert!(EngineCensus::on_this_thread().since(local).is_empty());
    }
}
