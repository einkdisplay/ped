use std::time::{Duration, Instant};

/// Native Kindle Paperwhite 3-class panel size (portrait).
pub const NATIVE_WIDTH: u32 = 1072;
pub const NATIVE_HEIGHT: u32 = 1448;

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

    pub fn from_css(
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        frame_w: u32,
        frame_h: u32,
    ) -> Option<Self> {
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

/// Logical page orientation relative to the native portrait panel.
///
/// Landscape modes render the page at swapped dimensions and rotate into the
/// physical framebuffer on commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Orientation {
    /// Native panel orientation (1072×1448).
    Portrait,
    /// Clockwise 90° from portrait (logical 1448×1072).
    Landscape,
    /// 180° from portrait.
    PortraitInverted,
    /// Counter-clockwise 90° from portrait / CW 270° (logical 1448×1072).
    LandscapeInverted,
}

impl Orientation {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "portrait" | "0" => Ok(Self::Portrait),
            "landscape" | "90" | "landscape-cw" => Ok(Self::Landscape),
            "portrait-inverted" | "inverted" | "180" | "portrait-flipped" => {
                Ok(Self::PortraitInverted)
            }
            "landscape-inverted" | "270" | "landscape-ccw" | "landscape-flipped" => {
                Ok(Self::LandscapeInverted)
            }
            _ => Err(format!(
                "unsupported display.orientation: {value} (expected portrait, landscape, portrait-inverted, or landscape-inverted)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
            Self::PortraitInverted => "portrait-inverted",
            Self::LandscapeInverted => "landscape-inverted",
        }
    }

    /// Servo / page viewport size for this orientation.
    pub fn logical_size(self) -> (u32, u32) {
        match self {
            Self::Portrait | Self::PortraitInverted => (NATIVE_WIDTH, NATIVE_HEIGHT),
            Self::Landscape | Self::LandscapeInverted => (NATIVE_HEIGHT, NATIVE_WIDTH),
        }
    }

    /// Map a dirty region from logical page coordinates into native FB space.
    pub fn transform_rect(self, region: Rect) -> Rect {
        let (lw, lh) = self.logical_size();
        match self {
            Self::Portrait => region,
            Self::Landscape => {
                // (x, y) -> (lh - 1 - y, x)
                Rect {
                    left: lh.saturating_sub(region.top + region.height),
                    top: region.left,
                    width: region.height,
                    height: region.width,
                }
            }
            Self::PortraitInverted => Rect {
                left: lw.saturating_sub(region.left + region.width),
                top: lh.saturating_sub(region.top + region.height),
                width: region.width,
                height: region.height,
            },
            Self::LandscapeInverted => {
                // (x, y) -> (y, lw - 1 - x)
                Rect {
                    left: region.top,
                    top: lw.saturating_sub(region.left + region.width),
                    width: region.height,
                    height: region.width,
                }
            }
        }
    }
}

/// Rotate a logical grayscale frame into native panel coordinates.
pub fn rotate_to_native(frame: &GrayFrame, orientation: Orientation) -> Result<GrayFrame, String> {
    let (lw, lh) = orientation.logical_size();
    if frame.width != lw || frame.height != lh {
        return Err(format!(
            "frame is {}x{}, expected logical {}x{} for {}",
            frame.width,
            frame.height,
            lw,
            lh,
            orientation.as_str()
        ));
    }
    match orientation {
        Orientation::Portrait => Ok(GrayFrame {
            width: frame.width,
            height: frame.height,
            pixels: frame.pixels.clone(),
        }),
        Orientation::Landscape => {
            let mut pixels = vec![0_u8; (NATIVE_WIDTH * NATIVE_HEIGHT) as usize];
            for y in 0..lh {
                for x in 0..lw {
                    let src = (y * lw + x) as usize;
                    let dx = lh - 1 - y;
                    let dy = x;
                    pixels[(dy * NATIVE_WIDTH + dx) as usize] = frame.pixels[src];
                }
            }
            GrayFrame::new(NATIVE_WIDTH, NATIVE_HEIGHT, pixels)
        }
        Orientation::PortraitInverted => {
            let mut pixels = vec![0_u8; (lw * lh) as usize];
            for y in 0..lh {
                for x in 0..lw {
                    let src = (y * lw + x) as usize;
                    let dx = lw - 1 - x;
                    let dy = lh - 1 - y;
                    pixels[(dy * lw + dx) as usize] = frame.pixels[src];
                }
            }
            GrayFrame::new(lw, lh, pixels)
        }
        Orientation::LandscapeInverted => {
            let mut pixels = vec![0_u8; (NATIVE_WIDTH * NATIVE_HEIGHT) as usize];
            for y in 0..lh {
                for x in 0..lw {
                    let src = (y * lw + x) as usize;
                    let dx = y;
                    let dy = lw - 1 - x;
                    pixels[(dy * NATIVE_WIDTH + dx) as usize] = frame.pixels[src];
                }
            }
            GrayFrame::new(NATIVE_WIDTH, NATIVE_HEIGHT, pixels)
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
        let mut max_x = 0_u32;
        let mut max_y = 0_u32;
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
        if !changed {
            return None;
        }
        Some(Rect {
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
        // First pending refresh is due immediately (no prior commit).
        scheduler.request(false);
        assert!(scheduler.due(start));
        assert_eq!(scheduler.commit(start), 1);
        // Subsequent requests respect the interval.
        scheduler.request(false);
        assert!(!scheduler.due(start));
        assert!(scheduler.due(start + Duration::from_secs(1)));
        assert_eq!(scheduler.commit(start + Duration::from_secs(1)), 2);
        assert!(!scheduler.due(start + Duration::from_secs(1)));
    }

    #[test]
    fn waveform_rejects_unknown_values() {
        assert!(Waveform::parse("bad").is_err());
    }

    #[test]
    fn orientation_aliases_parse() {
        assert_eq!(
            Orientation::parse("portrait").unwrap(),
            Orientation::Portrait
        );
        assert_eq!(Orientation::parse("90").unwrap(), Orientation::Landscape);
        assert_eq!(
            Orientation::parse("landscape-ccw").unwrap(),
            Orientation::LandscapeInverted
        );
        assert!(Orientation::parse("sideways").is_err());
    }

    #[test]
    fn landscape_logical_size_swaps_native() {
        assert_eq!(
            Orientation::Portrait.logical_size(),
            (NATIVE_WIDTH, NATIVE_HEIGHT)
        );
        assert_eq!(
            Orientation::Landscape.logical_size(),
            (NATIVE_HEIGHT, NATIVE_WIDTH)
        );
    }

    #[test]
    fn rotate_landscape_maps_corners() {
        let region = Rect {
            left: 10,
            top: 20,
            width: 30,
            height: 40,
        };
        assert_eq!(
            Orientation::Landscape.transform_rect(region),
            Rect {
                left: NATIVE_WIDTH - (20 + 40),
                top: 10,
                width: 40,
                height: 30,
            }
        );
        assert_eq!(
            Orientation::LandscapeInverted.transform_rect(region),
            Rect {
                left: 20,
                top: NATIVE_HEIGHT - (10 + 30),
                width: 40,
                height: 30,
            }
        );
    }

    #[test]
    fn rotate_portrait_identity_preserves_pixels() {
        let frame = GrayFrame::new(
            NATIVE_WIDTH,
            NATIVE_HEIGHT,
            vec![7; (NATIVE_WIDTH * NATIVE_HEIGHT) as usize],
        )
        .unwrap();
        let rotated = rotate_to_native(&frame, Orientation::Portrait).unwrap();
        assert_eq!(rotated, frame);
    }

    #[test]
    fn rotate_landscape_moves_top_left_pixel() {
        // Tiny stand-in: build a full-size frame with one marked pixel at (0,0).
        let mut pixels = vec![0_u8; (NATIVE_HEIGHT * NATIVE_WIDTH) as usize];
        pixels[0] = 255; // logical (0,0) in landscape 1448x1072
        let frame = GrayFrame::new(NATIVE_HEIGHT, NATIVE_WIDTH, pixels).unwrap();
        let rotated = rotate_to_native(&frame, Orientation::Landscape).unwrap();
        // CW: (0,0) -> (lh-1, 0) = (NATIVE_WIDTH-1, 0)
        let idx = (0 * NATIVE_WIDTH + (NATIVE_WIDTH - 1)) as usize;
        assert_eq!(rotated.pixels[idx], 255);
        assert_eq!(rotated.width, NATIVE_WIDTH);
        assert_eq!(rotated.height, NATIVE_HEIGHT);
    }
}
