use std::time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Starting,
    Running,
    Refreshing,
    Stopping,
    Recovering,
    Stopped,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Refreshing => "refreshing",
            Self::Stopping => "stopping",
            Self::Recovering => "recovering",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RefreshStats {
    pub sequence: u64,
    pub successful: u64,
    pub failed: u64,
    pub last_region: Option<(u32, u32, u32, u32)>,
    pub last_refresh: Option<Instant>,
}

impl Default for RefreshStats {
    fn default() -> Self {
        Self {
            sequence: 0,
            successful: 0,
            failed: 0,
            last_region: None,
            last_refresh: None,
        }
    }
}

impl RefreshStats {
    pub fn record_success(&mut self, sequence: u64, region: (u32, u32, u32, u32), at: Instant) {
        self.sequence = sequence;
        self.successful += 1;
        self.last_region = Some(region);
        self.last_refresh = Some(at);
    }

    pub fn record_failure(&mut self) {
        self.failed += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_names_are_stable() {
        assert_eq!(State::Refreshing.as_str(), "refreshing");
    }
}
