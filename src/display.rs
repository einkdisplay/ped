use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            left: 0,
            top: 0,
            width,
            height,
        }
    }

    pub fn from_css(x: f64, y: f64, width: f64, height: f64, frame_w: u32, frame_h: u32) -> Option<Self> {
        if width <= 0.0 || height <= 0.0 || frame_w == 0 || frame_h == 0 {
            return None;
        }
        let left = x.floor().max(0.0) as u32;
        let top = y.floor().max(0.0) as u32;
        let right = (x + width).ceil().max(0.0) as u32;
        let bottom = (y + height).ceil().max(0.0) as u32;
        let left = left.min(frame_w.saturating_sub(1));
        let top = top.min(frame_h.saturating_sub(1));
        let right = right.min(frame_w).max(left + 1);
        let bottom = bottom.min(frame_h).max(top + 1);
        Some(Self {
            left,
            top,
            width: right - left,
            height: bottom - top,
        })
    }

    pub fn union(self, other: Self) -> Self {
        let left = self.left.min(other.left);
        let top = self.top.min(other.top);
        let right = (self.left + self.width).max(other.left + other.width);
        let bottom = (self.top + self.height).max(other.top + other.height);
        Self {
            left,
            top,
            width: right - left,
            height: bottom - top,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GrayFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl GrayFrame {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, String> {
        let expected = width
            .checked_mul(height)
            .ok_or_else(|| "frame dimensions overflow".to_owned())? as usize;
        if pixels.len() != expected {
            return Err(format!(
                "frame has {} pixels, expected {expected}",
                pixels.len()
            ));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn changed_region(&self, previous: Option<&GrayFrame>) -> Option<Rect> {
        let previous = previous?;
        if previous.width != self.width || previous.height != self.height {
            return Some(Rect::full(self.width, self.height));
        }

        let mut min_x = self.width;
        let mut min_y = self.height;
        let mut max_x = 0;
        let mut max_y = 0;
        let mut changed = false;
        for (index, (&current, &old)) in self.pixels.iter().zip(previous.pixels.iter()).enumerate()
        {
            if current == old {
                continue;
            }
            changed = true;
            let index = index as u32;
            let x = index % self.width;
            let y = index / self.width;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        changed.then_some(Rect {
            left: min_x,
            top: min_y,
            width: max_x - min_x + 1,
            height: max_y - min_y + 1,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Waveform {
    Auto,
    Fast,
    Quality,
}

impl Waveform {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Auto),
            "fast" => Ok(Self::Fast),
            "quality" => Ok(Self::Quality),
            _ => Err(format!("unsupported display waveform: {value}")),
        }
    }
}

#[derive(Debug)]
pub struct Scheduler {
    enabled: bool,
    interval: Duration,
    last_refresh: Option<Instant>,
    pending: bool,
    pending_full: bool,
    sequence: u64,
}

impl Scheduler {
    pub fn new(enabled: bool, interval_ms: u64) -> Self {
        Self {
            enabled,
            interval: Duration::from_millis(interval_ms),
            last_refresh: None,
            pending: false,
            pending_full: false,
            sequence: 0,
        }
    }

    pub fn request(&mut self, full: bool) {
        self.pending = true;
        self.pending_full |= full;
    }

    pub fn due(&self, now: Instant) -> bool {
        self.pending
            && (!self.enabled
                || self
                    .last_refresh
                    .is_none_or(|last| now.duration_since(last) >= self.interval))
    }

    pub fn commit(&mut self, now: Instant) -> u64 {
        self.pending = false;
        self.pending_full = false;
        self.last_refresh = Some(now);
        self.sequence += 1;
        self.sequence
    }

    pub fn take_full_request(&mut self) -> bool {
        let full = self.pending_full;
        self.pending_full = false;
        full
    }

    pub fn full_requested(&self) -> bool {
        self.pending_full
    }

    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn set_interval_ms(&mut self, interval_ms: u64) {
        self.interval = Duration::from_millis(interval_ms.max(1));
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn interval_ms(&self) -> u64 {
        self.interval.as_millis() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_region_is_the_bounding_union() {
        let old = GrayFrame::new(4, 3, vec![0; 12]).unwrap();
        let mut pixels = vec![0; 12];
        pixels[1] = 1;
        pixels[10] = 2;
        let current = GrayFrame::new(4, 3, pixels).unwrap();
        assert_eq!(
            current.changed_region(Some(&old)),
            Some(Rect {
                left: 1,
                top: 0,
                width: 2,
                height: 3
            })
        );
    }

    #[test]
    fn identical_frames_do_not_need_refresh() {
        let frame = GrayFrame::new(2, 2, vec![0; 4]).unwrap();
        assert_eq!(frame.changed_region(Some(&frame)), None);
    }

    #[test]
    fn scheduler_coalesces_until_due() {
        let mut scheduler = Scheduler::new(true, 1000);
        let start = Instant::now();
        scheduler.request(false);
        assert!(!scheduler.due(start));
        assert!(scheduler.due(start + Duration::from_secs(1)));
        assert_eq!(scheduler.commit(start + Duration::from_secs(1)), 1);
        assert!(!scheduler.due(start + Duration::from_secs(1)));
    }

    #[test]
    fn waveform_rejects_unknown_values() {
        assert!(Waveform::parse("bad").is_err());
    }
}
