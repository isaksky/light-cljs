//! Checked-in QuickJS snapshot, split by consumer. Regeneration and coverage:
//! scripts/quickjs-data/README.md and docs/architecture.md, "Intl locale data".

pub(crate) mod collation;
pub mod dates;
pub mod display_names;
pub mod likely_subtags;
pub(crate) mod lists;
pub mod locale;
pub mod numbers;
pub mod preferences;
pub(crate) mod relative_time;

pub(crate) mod units;
