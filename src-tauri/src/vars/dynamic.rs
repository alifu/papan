//! Dynamic variables (`{{$uuid}}` and friends, plan 04 §5 rule 2). Each occurrence is
//! evaluated separately.

use std::collections::BTreeMap;

use time::OffsetDateTime;

/// Every dynamic name, sorted.
pub const DYNAMIC_NAMES: [&str; 6] = [
    "$guid",
    "$isoTimestamp",
    "$randomInt",
    "$randomUUID",
    "$timestamp",
    "$uuid",
];

/// Upper bound (inclusive) of `$randomInt`.
const RANDOM_INT_MAX: u128 = 1000;

pub trait Dynamics: Send + Sync {
    /// The value of the dynamic variable `name` (with its `$`), or `None` if unknown.
    fn value(&self, name: &str) -> Option<String>;
}

/// Real clock and randomness.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemDynamics;

impl Dynamics for SystemDynamics {
    fn value(&self, name: &str) -> Option<String> {
        match name {
            "$uuid" | "$guid" | "$randomUUID" => Some(uuid::Uuid::new_v4().to_string()),
            "$timestamp" => Some(OffsetDateTime::now_utc().unix_timestamp().to_string()),
            "$isoTimestamp" => Some(iso_millis(OffsetDateTime::now_utc())),
            "$randomInt" => {
                let random = uuid::Uuid::new_v4().as_u128() % (RANDOM_INT_MAX + 1);
                Some(random.to_string())
            }
            _ => None,
        }
    }
}

/// Fixed values, for tests.
#[derive(Debug, Clone, Default)]
pub struct FixedDynamics(pub BTreeMap<String, String>);

impl Dynamics for FixedDynamics {
    fn value(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

/// RFC 3339 in UTC with milliseconds, e.g. `2026-11-12T08:30:00.123Z`.
fn iso_millis(at: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
        at.millisecond()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(name: &str) -> String {
        SystemDynamics.value(name).unwrap()
    }

    #[test]
    fn values_have_the_right_shape_and_differ_per_call() {
        for name in ["$uuid", "$guid", "$randomUUID"] {
            assert!(uuid::Uuid::parse_str(&value(name)).is_ok(), "{name}");
            assert_ne!(value(name), value(name), "{name}");
        }
        assert!(value("$timestamp").parse::<i64>().unwrap() > 1_700_000_000);
        let number: u32 = value("$randomInt").parse().unwrap();
        assert!(number <= 1000);
        let iso = value("$isoTimestamp");
        assert_eq!(iso.len(), "2026-11-12T08:30:00.123Z".len(), "{iso}");
        assert!(iso.ends_with('Z') && iso.as_bytes()[10] == b'T', "{iso}");
        assert_eq!(SystemDynamics.value("$nope"), None);
    }

    #[test]
    fn every_listed_name_is_known() {
        for name in DYNAMIC_NAMES {
            assert!(SystemDynamics.value(name).is_some(), "{name}");
        }
    }

    #[test]
    fn iso_format_pads_fields() {
        let at = OffsetDateTime::from_unix_timestamp(0).unwrap();
        assert_eq!(iso_millis(at), "1970-01-01T00:00:00.000Z");
    }
}
