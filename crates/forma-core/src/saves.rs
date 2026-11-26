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

/// Each user's own saves (#301): Batocera's emulators all keep theirs under `saves/`, so the
/// folder of the user playing is bind-mounted over it while they are in. The admin's are the
/// folder itself, as before users; another user's are `saves/.forma-users/<id>`, out of sight
/// while theirs is mounted.
pub struct UserSaves {
    shell: Box<dyn crate::host::Shell>,
    saves: std::path::PathBuf,
    mountinfo: std::path::PathBuf,
}

/// The folder in `saves/` that holds the users' own.
const USERS_DIR: &str = ".forma-users";

impl UserSaves {
    pub fn new(shell: Box<dyn crate::host::Shell>, saves: &Path) -> Self {
        Self {
            shell,
            saves: saves.to_path_buf(),
            mountinfo: "/proc/self/mountinfo".into(),
        }
    }

    /// For tests: where the mounts are read from.
    pub fn with_mountinfo(mut self, mountinfo: &Path) -> Self {
        self.mountinfo = mountinfo.to_path_buf();
        self
    }

    /// `saves/` as user `user`'s from now on; the admin's the folder itself.
    pub fn switch(&self, user: i64) -> crate::Result<()> {
        // Ours taken off first, as many as are on (one, unless Forma stopped halfway).
        for _ in 0..4 {
            if !self.ours_mounted() {
                break;
            }
            let saves = self.saves.to_string_lossy();
            self.shell.run("umount", &[&saves])?;
        }
        if user == crate::library::ADMIN {
            return Ok(());
        }
        let own = self.saves.join(USERS_DIR).join(user.to_string());
        std::fs::create_dir_all(&own).map_err(|e| crate::Error::io(&own, e))?;
        let (own, saves) = (own.to_string_lossy(), self.saves.to_string_lossy());
        self.shell.run("mount", &["--bind", &own, &saves])?;
        Ok(())
    }

    /// Whether a user's folder of ours is mounted on `saves/`: the mount's root (its 4th
    /// field) is in `.forma-users`, its mount point (5th) `saves/`.
    fn ours_mounted(&self) -> bool {
        let Ok(text) = std::fs::read_to_string(&self.mountinfo) else {
            return false;
        };
        let point = self.saves.to_string_lossy();
        text.lines().any(|line| {
            let mut fields = line.split(' ').skip(3);
            let (Some(root), Some(at)) = (fields.next(), fields.next()) else {
                return false;
            };
            at == point && root.contains(&format!("/{USERS_DIR}/"))
        })
    }
}

#[cfg(test)]
mod user_saves_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<String>>>);

    impl crate::host::Shell for Recorder {
        fn run(&self, program: &str, args: &[&str]) -> crate::Result<String> {
            self.0
                .lock()
                .unwrap()
                .push(format!("{program} {}", args.join(" ")));
            Ok(String::new())
        }
    }

    #[test]
    fn a_users_folder_is_mounted_over_saves_and_the_admins_is_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let saves = dir.path().join("saves");
        let info = dir.path().join("mountinfo");
        let ran = Recorder::default();
        let user = UserSaves::new(Box::new(ran.clone()), &saves).with_mountinfo(&info);
        std::fs::write(&info, "22 1 0:1 / / rw - ext4 /dev/root rw\n").unwrap();

        user.switch(2).unwrap();
        assert!(saves.join(".forma-users/2").is_dir());
        let s = saves.to_string_lossy();
        assert_eq!(
            *ran.0.lock().unwrap(),
            [format!("mount --bind {s}/.forma-users/2 {s}")]
        );

        // Mounted now: the admin takes it off, and mounts nothing.
        std::fs::write(
            &info,
            format!("22 1 0:1 / / rw - ext4 /dev/root rw\n40 22 8:3 /saves/.forma-users/2 {s} rw - ext4 /dev/sda3 rw\n"),
        )
        .unwrap();
        ran.0.lock().unwrap().clear();
        // (The fake umount does not change the mounts: tried a few times, then on.)
        user.switch(1).unwrap();
        let calls = ran.0.lock().unwrap().clone();
        assert!(!calls.is_empty() && calls.iter().all(|c| *c == format!("umount {s}")));
    }
}
