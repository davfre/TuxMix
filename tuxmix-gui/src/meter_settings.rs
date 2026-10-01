//! Level meter preferences (Options → meters), saved as JSON next to the
//! scenes directory, like `layouts.rs`'s files. Read and written through
//! `serde_json::Value` so this crate needs no serde derive of its own.

use std::path::PathBuf;

/// The K-System scales (Bob Katz): 0 on the meter sits this many dB below
/// full scale, the RMS is AES17-calibrated (a full-scale sine reads full
/// scale) and averaged over 600 ms.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KScale {
    /// Plain dBFS.
    #[default]
    Off,
    K20,
    K14,
    K12,
}

impl KScale {
    pub const ALL: [KScale; 4] = [KScale::Off, KScale::K20, KScale::K14, KScale::K12];

    /// Where the scale's 0 sits, in dBFS. `None` for plain dBFS.
    pub fn reference_db(self) -> Option<f32> {
        match self {
            KScale::Off => None,
            KScale::K20 => Some(-20.0),
            KScale::K14 => Some(-14.0),
            KScale::K12 => Some(-12.0),
        }
    }

    fn key(self) -> &'static str {
        match self {
            KScale::Off => "dBFS",
            KScale::K20 => "K-20",
            KScale::K14 => "K-14",
            KScale::K12 => "K-12",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

impl std::fmt::Display for KScale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.key())
    }
}

/// A peak hold time in seconds, as the Options pick list shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeakHold(pub f32);

impl PeakHold {
    /// The choices offered: TotalMix allows 0.1 to 9.9 s.
    pub const CHOICES: [PeakHold; 6] = [
        PeakHold(0.5),
        PeakHold(1.0),
        PeakHold(2.0),
        PeakHold(3.0),
        PeakHold(5.0),
        PeakHold(9.9),
    ];
}

impl std::fmt::Display for PeakHold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "hold {} s", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterSettings {
    /// Lift RMS by 3 dB so a full-scale sine reads 0 dBFS on RMS as on
    /// peak (the AES17 convention). Always on with a K-scale.
    pub rms_plus3: bool,
    /// How long a new peak holds before it falls, in seconds.
    pub peak_hold_s: f32,
    pub k_scale: KScale,
}

impl Default for MeterSettings {
    fn default() -> Self {
        Self {
            rms_plus3: false,
            peak_hold_s: 1.0,
            k_scale: KScale::Off,
        }
    }
}

impl MeterSettings {
    /// Whether RMS is shown with the AES17 +3 dB: by choice, or because a
    /// K-scale requires it.
    pub fn rms_aes17(&self) -> bool {
        self.rms_plus3 || self.k_scale != KScale::Off
    }

    /// The RMS display's time constant: 600 ms on a K-scale, 300 ms
    /// otherwise.
    pub fn rms_tau_ms(&self) -> f32 {
        if self.k_scale == KScale::Off {
            300.0
        } else {
            600.0
        }
    }

    pub fn load() -> Self {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .map(|v| Self::from_json(&v))
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.to_json()).map_err(|e| e.to_string())?;
        std::fs::write(path(), json).map_err(|e| e.to_string())
    }

    fn to_json(self) -> serde_json::Value {
        serde_json::json!({
            "rms_plus3": self.rms_plus3,
            "peak_hold_s": self.peak_hold_s,
            "k_scale": self.k_scale.key(),
        })
    }

    /// Missing or unreadable fields keep their defaults.
    fn from_json(v: &serde_json::Value) -> Self {
        let d = Self::default();
        Self {
            rms_plus3: v["rms_plus3"].as_bool().unwrap_or(d.rms_plus3),
            peak_hold_s: v["peak_hold_s"]
                .as_f64()
                .map(|s| (s as f32).clamp(0.1, 9.9))
                .unwrap_or(d.peak_hold_s),
            k_scale: v["k_scale"]
                .as_str()
                .and_then(KScale::from_key)
                .unwrap_or(d.k_scale),
        }
    }
}

fn path() -> PathBuf {
    let dir = crate::scenes::scenes_dir()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("meters.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_json() {
        let s = MeterSettings {
            rms_plus3: true,
            peak_hold_s: 3.0,
            k_scale: KScale::K14,
        };
        assert_eq!(MeterSettings::from_json(&s.to_json()), s);
    }

    #[test]
    fn a_missing_or_bad_field_keeps_its_default() {
        let v = serde_json::json!({ "k_scale": "K-99", "peak_hold_s": 99.0 });
        let s = MeterSettings::from_json(&v);
        assert_eq!(s.k_scale, KScale::Off);
        assert_eq!(s.peak_hold_s, 9.9);
        assert!(!s.rms_plus3);
    }

    #[test]
    fn a_k_scale_implies_aes17_rms_and_a_slower_average() {
        let s = MeterSettings {
            k_scale: KScale::K20,
            ..MeterSettings::default()
        };
        assert!(s.rms_aes17());
        assert_eq!(s.rms_tau_ms(), 600.0);
        assert_eq!(KScale::K20.reference_db(), Some(-20.0));
    }
}
