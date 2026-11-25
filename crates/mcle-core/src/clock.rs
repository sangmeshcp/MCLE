use mcle_sdk::Clock;
use std::sync::{Arc, Mutex};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

#[derive(Debug)]
pub struct ManualClock(Mutex<OffsetDateTime>);

impl ManualClock {
    pub fn new(t: OffsetDateTime) -> Self {
        ManualClock(Mutex::new(t))
    }
}

impl Clock for ManualClock {
    fn now(&self) -> OffsetDateTime {
        match self.0.lock() {
            Ok(g) => *g,
            Err(p) => *p.into_inner(),
        }
    }
    fn advance(&self, by: time::Duration) {
        let mut g = match self.0.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *g += by;
    }
}

pub fn from_spec(spec: &str) -> Result<Arc<dyn Clock>, String> {
    if spec.is_empty() || spec == "real" {
        return Ok(Arc::new(SystemClock));
    }
    if let Some(ts) = spec.strip_prefix("fixed:") {
        let t =
            OffsetDateTime::parse(ts, &Rfc3339).map_err(|e| format!("bad clock `{spec}`: {e}"))?;
        return Ok(Arc::new(ManualClock::new(t)));
    }
    Err(format!(
        "bad clock `{spec}` (expected real|fixed:<rfc3339>)"
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_advances_only_manually() {
        let c = from_spec("fixed:2024-01-01T00:00:00Z").unwrap();
        let t0 = c.now();
        assert_eq!(c.now(), t0);
        c.advance(time::Duration::seconds(5));
        assert_eq!(c.now() - t0, time::Duration::seconds(5));
        assert!(from_spec("nope").is_err());
    }
}
