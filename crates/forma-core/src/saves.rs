//! A game's saves, as RetroArch keeps them under Batocera: in `saves/<system>/`, named after the
//! ROM's file (without its folder or extension), a battery save (`.srm`, `.sav`) and save states
//! (`.state`, `.state1`…, `.state.auto`; each with a `.png` beside it, which is not a save).
//! Standalone emulators keep theirs elsewhere, each its own way: not counted.

use std::path::Path;
use std::time::SystemTime;

use serde::Serialize;

/// What a game has saved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saves {
    /// A battery save: the game's own save, as on the cartridge.
    pub battery: bool,
    /// Save states, the automatic one included.
    pub states: u32,
    /// When the latest was written (ISO 8601, UTC); `None` with none.
    pub latest: Option<String>,
}

impl Saves {
    pub fn any(&self) -> bool {
        self.battery || self.states > 0
    }
}

/// The saves in `saves` (Batocera's saves folder) for the ROM at `rom` (its path in its
/// system's folder) of `system`.
pub fn of(saves: &Path, system: &str, rom: &str) -> Saves {
    let file = rom.rsplit('/').next().unwrap_or(rom);
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let mut out = Saves::default();
    let mut latest: Option<SystemTime> = None;
    let Ok(entries) = std::fs::read_dir(saves.join(system)) else {
        return out;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Some(rest) = name.strip_prefix(stem).and_then(|r| r.strip_prefix('.')) else {
            continue;
        };
        let state = rest == "state"
            || rest == "state.auto"
            || rest
                .strip_prefix("state")
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        let battery = rest == "srm" || rest == "sav";
        if !state && !battery {
            continue;
        }
        out.battery |= battery;
        out.states += u32::from(state);
        if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
            latest = latest.max(Some(modified));
        }
    }
    out.latest = latest.map(crate::time::iso_utc);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_a_roms_battery_save_and_states_not_their_pictures_nor_other_games() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("megadrive");
        std::fs::create_dir_all(&md).unwrap();
        for name in [
            "Sonic (USA).srm",
            "Sonic (USA).state",
            "Sonic (USA).state1",
            "Sonic (USA).state1.png",
            "Sonic (USA).state.auto",
            "Sonic (USA) (Rev 1).state2",
            "Sonic (USA).stateX",
            "Streets of Rage (USA).srm",
        ] {
            std::fs::write(md.join(name), "").unwrap();
        }
        let sonic = of(dir.path(), "megadrive", "sub/Sonic (USA).md");
        assert!(sonic.battery);
        assert_eq!(sonic.states, 3);
        assert!(sonic.any());
        assert!(sonic.latest.is_some());

        let none = of(dir.path(), "megadrive", "Golden Axe (World).md");
        assert_eq!(none, Saves::default());
        assert!(!none.any());
        assert_eq!(of(dir.path(), "snes", "Mario.sfc"), Saves::default());
    }
}
