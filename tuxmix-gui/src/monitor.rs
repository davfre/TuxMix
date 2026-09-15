//! Software monitor actions; the driver reports button presses only.
use serde::{Deserialize, Serialize};
use tuxmix_core::{ChannelId, RmeDevice, Scene};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Action {
    #[default]
    None,
    DimMain,
    DimMonitors,
    MuteMain,
    MuteMonitors,
    GlobalMute,
    Snapshot1,
    Snapshot2,
    Snapshot3,
    Snapshot4,
    Snapshot5,
    Snapshot6,
    Snapshot7,
    Snapshot8,
}
impl Action {
    pub const ALL: [Self; 14] = [
        Self::None,
        Self::DimMain,
        Self::DimMonitors,
        Self::MuteMain,
        Self::MuteMonitors,
        Self::GlobalMute,
        Self::Snapshot1,
        Self::Snapshot2,
        Self::Snapshot3,
        Self::Snapshot4,
        Self::Snapshot5,
        Self::Snapshot6,
        Self::Snapshot7,
        Self::Snapshot8,
    ];
    pub fn snapshot(self) -> Option<u8> {
        match self {
            Self::Snapshot1 => Some(1),
            Self::Snapshot2 => Some(2),
            Self::Snapshot3 => Some(3),
            Self::Snapshot4 => Some(4),
            Self::Snapshot5 => Some(5),
            Self::Snapshot6 => Some(6),
            Self::Snapshot7 => Some(7),
            Self::Snapshot8 => Some(8),
            _ => None,
        }
    }
}
impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(n) = self.snapshot() {
            return write!(f, "Snapshot {n}");
        }
        f.write_str(match self {
            Self::None => "No action",
            Self::DimMain => "DIM Main",
            Self::DimMonitors => "DIM monitors",
            Self::MuteMain => "Mute Main",
            Self::MuteMonitors => "Mute monitors",
            Self::GlobalMute => "Global mute",
            _ => unreachable!(),
        })
    }
}
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
    pub dim_db: u8,
    pub action: Action,
    pub exclusion: Exclusion,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            main: 0,
            monitors: [true, true, false, false, false, false],
            dim_db: 20,
            action: Action::None,
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
        c.dim_db = c.dim_db.clamp(1, 60);
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
#[derive(Default)]
pub struct Runtime {
    saved: Vec<(usize, f32, f32, bool)>,
    factor: Option<f32>,
}
impl Runtime {
    pub fn active(&self) -> bool {
        !self.saved.is_empty()
    }
    pub fn toggle(
        &mut self,
        device: &mut impl RmeDevice,
        config: &Config,
        all: bool,
        mute: bool,
    ) -> Result<(), String> {
        match self.toggle_inner(device, config, all, mute) {
            Ok(()) => Ok(()),
            Err(error) => {
                let mut muted = true;
                for pair in 0..device.outputs().len() / 2 {
                    if pair == config.main
                        || (all && config.monitors.get(pair).copied().unwrap_or(false))
                    {
                        muted &= device.set_mute(ChannelId::Output(pair * 2), true).is_ok();
                    }
                }
                self.saved.clear();
                self.factor = None;
                Err(format!(
                    "{error}. {}",
                    if muted {
                        "Selected outputs muted; check levels before continuing"
                    } else {
                        "Could not confirm output mute; keep speakers off"
                    }
                ))
            }
        }
    }

    fn toggle_inner(
        &mut self,
        device: &mut impl RmeDevice,
        config: &Config,
        all: bool,
        mute: bool,
    ) -> Result<(), String> {
        if self.active() {
            // Preserve wheel/fader changes made while dimmed instead of restoring stale levels.
            let current = device.outputs().to_vec();
            for &(i, base, applied, was_muted) in &self.saved {
                if let Some(factor) = self.factor {
                    let now = current.get(i).ok_or("Output layout changed")?.volume;
                    let restored = if (now - applied).abs() < f32::EPSILON * 4.0 {
                        base
                    } else {
                        (now / factor).clamp(0.0, 1.0)
                    };
                    device
                        .set_volume(ChannelId::Output(i), 0, restored)
                        .map_err(|e| e.to_string())?;
                } else if i % 2 == 0 {
                    device
                        .set_mute(ChannelId::Output(i), was_muted)
                        .map_err(|e| e.to_string())?;
                }
            }
            self.saved.clear();
            self.factor = None;
            return Ok(());
        }
        let factor = 10f32.powf(-(config.dim_db as f32) / 20.0);
        let outputs = device.outputs().to_vec();
        // Keep recovery information even if a write fails partway through.
        self.factor = if mute { None } else { Some(factor) };
        for (i, out) in outputs.iter().enumerate() {
            if i / 2 != config.main
                && !(all && config.monitors.get(i / 2).copied().unwrap_or(false))
            {
                continue;
            }
            let applied = out.volume * factor;
            self.saved.push((i, out.volume, applied, out.mute));
            if mute {
                if i % 2 == 0 {
                    device
                        .set_mute(ChannelId::Output(i), true)
                        .map_err(|e| e.to_string())?;
                }
            } else {
                device
                    .set_volume(ChannelId::Output(i), 0, applied)
                    .map_err(|e| e.to_string())?;
            }
        }
        device.poll_events().map_err(|e| e.to_string())?;
        for (i, _, applied, _) in &mut self.saved {
            *applied = device
                .outputs()
                .get(*i)
                .ok_or("Output layout changed")?
                .volume;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuxmix_core::MockBabyfacePro;
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
    #[test]
    fn dim_is_relative_restores_levels_and_leaves_digital_alone() {
        let mut dev = MockBabyfacePro::open().unwrap();
        let before = dev.capture_scene();
        let mut rt = Runtime::default();
        let cfg = Config::default();
        rt.toggle(&mut dev, &cfg, true, false).unwrap();
        for i in 0..4 {
            assert!((dev.outputs()[i].volume - before.outputs[i].volume * 0.1).abs() < 0.0001);
        }
        assert_eq!(dev.outputs()[4].volume, before.outputs[4].volume);
        rt.toggle(&mut dev, &cfg, true, false).unwrap();
        for i in 0..4 {
            assert_eq!(dev.outputs()[i].volume, before.outputs[i].volume);
        }
    }
    #[test]
    fn dim_restore_respects_a_changed_fader() {
        let mut dev = MockBabyfacePro::open().unwrap();
        let mut rt = Runtime::default();
        let cfg = Config::default();
        rt.toggle(&mut dev, &cfg, false, false).unwrap();
        dev.set_volume(ChannelId::Output(0), 0, 0.012).unwrap();
        rt.toggle(&mut dev, &cfg, false, false).unwrap();
        assert!((dev.outputs()[0].volume - 0.12).abs() < 0.0001);
    }
    #[test]
    fn monitor_mute_restores_preexisting_mutes() {
        let mut dev = MockBabyfacePro::open().unwrap();
        dev.set_mute(ChannelId::Output(0), true).unwrap();
        let mut rt = Runtime::default();
        let cfg = Config::default();
        rt.toggle(&mut dev, &cfg, true, true).unwrap();
        assert!(dev.outputs()[0].mute);
        assert!(dev.outputs()[2].mute);
        rt.toggle(&mut dev, &cfg, true, true).unwrap();
        assert!(dev.outputs()[0].mute);
        assert!(!dev.outputs()[2].mute);
    }
    #[test]
    fn digital_monitor_is_opt_in() {
        let mut dev = MockBabyfacePro::open().unwrap();
        let before = dev.capture_scene();
        let mut cfg = Config::default();
        cfg.monitors[2] = true;
        let mut rt = Runtime::default();
        rt.toggle(&mut dev, &cfg, true, false).unwrap();
        assert!((dev.outputs()[4].volume - before.outputs[4].volume * 0.1).abs() < 0.0001);
        assert_eq!(dev.outputs()[6].volume, before.outputs[6].volume);
    }
}
