//! Experimental CLI embedding; absent unless goose-macros is compiled in.
use light_cljs::{MacroHost, goose_evaluator::GooseSession};
use std::{error::Error, path::PathBuf, sync::Arc, time::Instant};

pub struct Backend(Option<Arc<GooseSession>>);
impl Backend {
    pub fn install(host: &mut MacroHost, cached: bool) -> Result<Self, Box<dyn Error>> {
        let Some(path) = std::env::var_os("LIGHT_CLJS_GOOSE_RUNNER") else {
            if std::env::var_os("LIGHT_CLJS_GOOSE_WORKERS").is_some() {
                return Err("LIGHT_CLJS_GOOSE_WORKERS requires LIGHT_CLJS_GOOSE_RUNNER".into());
            }
            return Ok(Self(None));
        };
        if cached {
            return Err("experimental Goose source macros require --no-cache; cached artifact installation is not validated".into());
        }
        let workers: usize = std::env::var("LIGHT_CLJS_GOOSE_WORKERS")
            .unwrap_or_else(|_| "1".into())
            .parse()?;
        if ![1, 2, 4, 8].contains(&workers) {
            return Err("LIGHT_CLJS_GOOSE_WORKERS must be 1, 2, 4 or 8".into());
        }
        let session = GooseSession::start(&PathBuf::from(path), workers)?;
        host.use_goose_source_macros(session.clone())?;
        Ok(Self(Some(session)))
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(session) = &self.0 {
            let started = Instant::now();
            session.close();
            if let Some(path) = std::env::var_os("LIGHT_CLJS_GOOSE_REPORT") {
                let text = format!(
                    "{:?}\ncleanup_ns {}\n{}\n",
                    session.stats(),
                    started.elapsed().as_nanos(),
                    session.trace().join("\n")
                );
                if let Err(error) = std::fs::write(path, text) {
                    eprintln!("Goose report: {error}");
                }
            }
        }
    }
}
