//! Level meters from the kernel driver.
//!
//! The Babyface Pro driver measures levels in its URB completion
//! handlers and exposes them as read-only PCM-interface controls
//! (`babyfacepro-meter.c` in babyface-pro-linux):
//!
//! | control                  | values                              |
//! |--------------------------|-------------------------------------|
//! | `Capture Meter Peak`     | largest \|sample\|, per PCM channel |
//! | `Capture Meter RMS`      | RMS, per PCM channel                |
//! | `Capture Meter Overs`    | longest full-scale run              |
//! | `Playback Meter Peak`    | the same for playback               |
//! | `Playback Meter RMS`     |                                     |
//! | `Playback Meter Overs`   |                                     |
//!
//! Each holds 12 values, 0 to [`FULL_SCALE`]. A read returns the level
//! since the previous read and starts a new interval, the same draining
//! convention as [`crate::RmeDevice::levels`], so read each control once
//! per UI tick. Another program reading the same controls (`amixer`,
//! `alsactl store`) takes part of the interval for itself.
//!
//! This needs no PCM stream of its own: the levels come from whatever
//! stream is already running, whether PipeWire or a DAW such as Bitwig
//! holds the device directly. With no stream running, every read is
//! zero.
//!
//! Older drivers do not have these controls. [`DriverMeters::open`]
//! probes for them, and the backend keeps showing N/A when they are
//! missing, so TuxMix and the driver can be updated independently.

use std::ffi::CString;

use alsa::ctl::{ElemId, ElemIface, ElemType, ElemValue};
use alsa::Ctl;
use log::{debug, info};

use crate::device::Level;

/// The driver's 0 dBFS: a 24-bit magnitude.
pub const FULL_SCALE: f32 = 0x7f_ffff as f32;

/// Values per meter control (PCM channels per direction).
pub const CHANNELS: usize = 12;

/// Meter direction, matching the driver's control name prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeterDir {
    Capture,
    Playback,
}

impl MeterDir {
    fn prefix(self) -> &'static str {
        match self {
            MeterDir::Capture => "Capture",
            MeterDir::Playback => "Playback",
        }
    }
}

/// A read-only handle on one direction's meter controls.
struct MeterPair {
    peak: ElemId,
    rms: ElemId,
}

impl MeterPair {
    fn new(dir: MeterDir) -> Self {
        let id = |kind: &str| {
            let mut id = ElemId::new(ElemIface::PCM);
            let name = CString::new(format!("{} Meter {kind}", dir.prefix()))
                .expect("control names contain no NUL");
            id.set_name(&name);
            id
        };
        Self {
            peak: id("Peak"),
            rms: id("RMS"),
        }
    }
}

/// The driver's meter controls on one card.
pub struct DriverMeters {
    ctl: Ctl,
    capture: MeterPair,
    playback: MeterPair,
}

impl DriverMeters {
    /// Open the controls on `card` (e.g. `"hw:1"`). `None` when the
    /// driver predates them or the card cannot be opened.
    ///
    /// The probe reads each control once, which drains it; harmless at
    /// startup.
    pub fn open(card: &str) -> Option<Self> {
        let ctl = match Ctl::new(card, false) {
            Ok(ctl) => ctl,
            Err(e) => {
                debug!("driver meters: cannot open {card}: {e}");
                return None;
            }
        };
        let meters = Self {
            ctl,
            capture: MeterPair::new(MeterDir::Capture),
            playback: MeterPair::new(MeterDir::Playback),
        };
        for pair in [&meters.capture, &meters.playback] {
            for id in [&pair.peak, &pair.rms] {
                if let Err(e) = meters.read_raw(id) {
                    info!(
                        "driver meters unavailable ({:?}: {e}); \
                         the loaded driver predates them",
                        id.get_name().unwrap_or("?")
                    );
                    return None;
                }
            }
        }
        info!("driver meters available on {card}");
        Some(meters)
    }

    fn read_raw(&self, id: &ElemId) -> Result<[i32; CHANNELS], alsa::Error> {
        let mut value = ElemValue::new(ElemType::Integer)?;
        value.set_id(id);
        self.ctl.elem_read(&mut value)?;
        let mut out = [0; CHANNELS];
        for (i, v) in out.iter_mut().enumerate() {
            *v = value.get_integer(i as u32).unwrap_or(0);
        }
        Ok(out)
    }

    /// Peak and RMS for every PCM channel in `dir` since the previous
    /// call, as 0..1 of full scale. `None` if the read fails, e.g. the
    /// card has gone away.
    pub fn read(&self, dir: MeterDir) -> Option<[Level; CHANNELS]> {
        let pair = match dir {
            MeterDir::Capture => &self.capture,
            MeterDir::Playback => &self.playback,
        };
        let peak = self.read_raw(&pair.peak).ok()?;
        let rms = self.read_raw(&pair.rms).ok()?;
        Some(levels_from_raw(&peak, &rms))
    }
}

/// Convert raw driver values to [`Level`]s.
pub fn levels_from_raw(peak: &[i32; CHANNELS], rms: &[i32; CHANNELS]) -> [Level; CHANNELS] {
    let scale = |v: i32| (v.max(0) as f32 / FULL_SCALE).min(1.0);
    std::array::from_fn(|i| Level {
        peak: scale(peak[i]),
        rms: scale(rms[i]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_values_scale_to_full_scale() {
        let mut peak = [0; CHANNELS];
        let mut rms = [0; CHANNELS];
        peak[0] = 0x7f_ffff;
        rms[0] = 0x5a_8279; // full-scale sine RMS, 1/sqrt(2)
        peak[1] = 0x40_0000; // -6 dBFS
        rms[2] = -5; // never negative from the driver; clamp anyway
        let l = levels_from_raw(&peak, &rms);
        assert_eq!(l[0].peak, 1.0);
        assert!((l[0].rms - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
        assert!((20.0 * l[1].peak.log10() + 6.02).abs() < 0.01);
        assert_eq!(l[2].rms, 0.0);
        assert_eq!(l[3], Level::default());
    }

    #[test]
    fn control_names_match_the_driver() {
        let c = MeterPair::new(MeterDir::Capture);
        let p = MeterPair::new(MeterDir::Playback);
        assert_eq!(c.peak.get_name().unwrap(), "Capture Meter Peak");
        assert_eq!(c.rms.get_name().unwrap(), "Capture Meter RMS");
        assert_eq!(p.peak.get_name().unwrap(), "Playback Meter Peak");
        assert_eq!(p.rms.get_name().unwrap(), "Playback Meter RMS");
        assert_eq!(c.peak.get_interface(), ElemIface::PCM);
    }
}
