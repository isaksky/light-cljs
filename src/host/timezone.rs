//! Immutable embedding zones backed by the pinned QuickJS timezone snapshot.
//! Only records with transitions (and their aliases) are accepted. First/last
//! offsets extend indefinitely: no future recurrence rules are present.
mod tables;

/// A validated runtime zone; named zones never silently fall back to UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeZone(Zone);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Zone {
    Fixed(i32),
    Named(usize),
}

impl TimeZone {
    pub const UTC: Self = Self(Zone::Fixed(0));

    /// Explicit fixed offsets retain seconds; magnitudes must be below 24h.
    pub fn fixed_offset_seconds(seconds: i32) -> Result<Self, String> {
        if (-86_399..86_400).contains(&seconds) {
            Ok(Self(Zone::Fixed(seconds)))
        } else {
            Err("time zone offset must be less than 24 hours".into())
        }
    }

    /// Resolve a snapshot canonical name or alias. The 598-name inventory is
    /// broader than the 59 records with rules; inventory-only names fail.
    pub fn named(name: &str) -> Result<Self, String> {
        Self::lookup_named(name, false)
    }

    /// ECMA-402 matches available zone identifiers without ASCII case.
    pub(crate) fn named_case_insensitive(name: &str) -> Result<Self, String> {
        Self::lookup_named(name, true)
    }

    fn lookup_named(name: &str, ignore_case: bool) -> Result<Self, String> {
        if ["UTC", "Etc/UTC", "Etc/GMT", "GMT"].iter().any(|n| {
            if ignore_case {
                n.eq_ignore_ascii_case(name)
            } else {
                *n == name
            }
        }) {
            return Ok(Self::UTC);
        }
        let matches = |bytes: Option<&[u8]>| {
            bytes.and_then(|s| s.strip_suffix(&[0])).is_some_and(|s| {
                if ignore_case {
                    s.eq_ignore_ascii_case(name.as_bytes())
                } else {
                    s == name.as_bytes()
                }
            })
        };
        let index = tables::ti_tz_zone_data
            .iter()
            .position(|z| matches(z.canonical_name))
            .or_else(|| {
                tables::ti_tz_aliases
                    .iter()
                    .find(|a| matches(a.alias))
                    .map(|a| a.canonical_id as usize)
            });
        match index {
            Some(i)
                if tables::ti_tz_zone_data
                    .get(i)
                    .is_some_and(|z| z.transitions.is_some_and(|t| !t.is_empty())) =>
            {
                Ok(Self(Zone::Named(i)))
            }
            _ => Err(format!(
                "time zone {name:?} has no snapshot transition data"
            )),
        }
    }

    /// Stable embedding-zone identifier used by Intl resolvedOptions.
    pub fn identifier(self) -> String {
        match self.0 {
            Zone::Fixed(0) => "UTC".into(),
            Zone::Fixed(seconds) => {
                let sign = if seconds < 0 { "-" } else { "+" };
                let n = seconds.abs();
                if n % 60 == 0 {
                    format!("{sign}{:02}:{:02}", n / 3600, n / 60 % 60)
                } else {
                    format!("{sign}{:02}:{:02}:{:02}", n / 3600, n / 60 % 60, n % 60)
                }
            }
            Zone::Named(i) => std::str::from_utf8(
                tables::ti_tz_zone_data[i]
                    .canonical_name
                    .unwrap()
                    .strip_suffix(&[0])
                    .unwrap(),
            )
            .expect("validated zone name")
            .into(),
        }
    }

    fn transitions(self) -> &'static [tables::TZTransition] {
        match self.0 {
            Zone::Named(i) => tables::ti_tz_zone_data[i].transitions.unwrap(),
            Zone::Fixed(_) => &[],
        }
    }

    /// Seconds east of UTC. Before/after the table use its first/last offset.
    pub fn offset_seconds(self, utc_millis: f64) -> i32 {
        match self.0 {
            Zone::Fixed(offset) => offset,
            Zone::Named(_) => {
                let transitions = self.transitions();
                let end =
                    transitions.partition_point(|t| t.start_sec as f64 * 1000.0 <= utc_millis);
                transitions[end.saturating_sub(1)].offset_sec
            }
        }
    }

    /// ECMA-262 LocalTime, without TimeClip (local fields may cross its edge).
    pub fn local_time_millis(self, utc_millis: f64) -> f64 {
        utc_millis + f64::from(self.offset_seconds(utc_millis)) * 1000.0
    }

    /// ECMA-262 UTC(t): overlaps choose the earlier instant; gaps use the
    /// pre-transition offset, advancing the wall time by the gap. This scans
    /// at most the finite snapshot's transition count, with no callbacks.
    pub fn utc_time_millis(self, local_millis: f64) -> f64 {
        if !local_millis.is_finite() {
            return f64::NAN;
        }
        if let Zone::Fixed(offset) = self.0 {
            return local_millis - f64::from(offset) * 1000.0;
        }
        let transitions = self.transitions();
        let mut earliest = f64::INFINITY;
        for (i, t) in transitions.iter().enumerate() {
            let start = if i == 0 {
                f64::NEG_INFINITY
            } else {
                t.start_sec as f64 * 1000.0
            };
            let end = transitions
                .get(i + 1)
                .map_or(f64::INFINITY, |next| next.start_sec as f64 * 1000.0);
            let candidate = local_millis - f64::from(t.offset_sec) * 1000.0;
            if candidate >= start && candidate < end {
                earliest = earliest.min(candidate);
            }
            if let Some(next) = transitions.get(i + 1) {
                let before = end + f64::from(t.offset_sec) * 1000.0;
                let after = end + f64::from(next.offset_sec) * 1000.0;
                if local_millis >= before && local_millis < after {
                    return candidate;
                }
            }
        }
        earliest
    }
}
