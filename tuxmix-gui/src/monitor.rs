//! Monitor output selection and snapshot recall exclusions.
use serde::{Deserialize, Serialize};
use tuxmix_core::Scene;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Exclusion {
    LoadAll,
    PreserveMain,
    #[default]
    PreserveMonitors,
}
impl Exclusion {
    pub const ALL: [Self; 3] = [Self::LoadAll, Self::PreserveMain, Self::PreserveMonitors];
}
impl std::fmt::Display for Exclusion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::LoadAll => "Load all levels",
            Self::PreserveMain => "Keep Main levels and mute",
            Self::PreserveMonitors => "Keep monitor levels and mutes",
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub main: usize,
    pub monitors: [bool; 6],
    pub exclusion: Exclusion,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            main: 0,
            monitors: [true, true, false, false, false, false],
            exclusion: Exclusion::PreserveMonitors,
        }
    }
}
impl Config {
    fn path() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".local/share/tuxmix/monitor.json")
    }
    pub fn load() -> Self {
        let mut c: Self = std::fs::read(Self::path())
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or_default();
        c.main = c.main.min(5);
        c
    }
    pub fn save(&self) -> Result<(), String> {
        let p = Self::path();
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        let tmp = p.with_extension("tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, p).map_err(|e| e.to_string())
    }
    pub fn protect_snapshot(&self, saved: &Scene, current: &Scene) -> Scene {
        let mut scene = saved.clone();
        for (i, out) in scene.outputs.iter_mut().enumerate() {
            let pair = i / 2;
            let protect = match self.exclusion {
                Exclusion::LoadAll => false,
                Exclusion::PreserveMain => pair == self.main,
                Exclusion::PreserveMonitors => {
                    pair == self.main || self.monitors.get(pair).copied().unwrap_or(false)
                }
            };
            if protect {
                if let Some(live) = current.outputs.get(i) {
                    out.volume = live.volume;
                    out.mute = live.mute;
                }
            }
        }
        scene
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use tuxmix_core::{ChannelId, RmeDevice, MockBabyfacePro};
    #[test]
    fn snapshot_preserves_only_selected_monitor_levels() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_volume(ChannelId::Output(0), 0, 0.02).unwrap();
        dev.set_mute(ChannelId::Output(0), true).unwrap();
        let current = dev.capture_scene();
        let mut saved = current.clone();
        for out in &mut saved.outputs {
            out.volume = 0.9;
            out.mute = false;
        }
        let restored = Config::default().protect_snapshot(&saved, &current);
        assert_eq!(restored.outputs[0].volume, current.outputs[0].volume);
        assert!(restored.outputs[0].mute);
        assert_eq!(restored.outputs[2].volume, current.outputs[2].volume);
        assert_eq!(restored.outputs[4].volume, 0.9);
    }
}
