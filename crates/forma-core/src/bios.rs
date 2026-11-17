//! BIOS files: which each system lists, and whether they are there with the right contents.
//!
//! The list is Batocera's own (`batocera-systems`, a Python script holding it as a dict), read
//! as data so it follows Batocera's updates; the check is Forma's. Batocera's check reports
//! every listed file missing, alternatives (one per region, one per model) and optional ones
//! alike, and skips systems whose name differs from the list's (`megacd`, listed `segacd`).
//! Forma tells a system that cannot start without one (`NEEDED`) from one that can.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use crate::host;

/// Batocera's list of BIOS files, in its script.
pub const BATOCERA_SYSTEMS: &str = "/usr/bin/batocera-systems";

/// Prints the script's `systems` as JSON, without running its check.
const DUMP: &str = "import json,sys,importlib.util as u,importlib.machinery as m\n\
l=m.SourceFileLoader('bs',sys.argv[1]);s=u.spec_from_loader('bs',l)\n\
mod=u.module_from_spec(s);l.exec_module(mod);print(json.dumps(mod.systems))";

/// A system Forma names otherwise than Batocera's list, and the list's name for it.
const LISTED_AS: &[(&str, &str)] = &[("megacd", "segacd"), ("pcenginecd", "pcengine")];

/// How a system's listed files are needed for any of its games to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Need {
    /// One of them: alternatives (a region's, a model's).
    Any,
    /// Every one.
    All,
}

/// Systems that do not start without a BIOS. Others list files a game may use (a CD add-on's
/// card for a console that also plays cartridges, a Super Game Boy's) or do without (a core's
/// own replacement, as PlayStation and Dreamcast have).
const NEEDED: &[(&str, Need)] = &[
    ("3do", Need::Any),
    ("atari5200", Need::Any),
    ("atomiswave", Need::Any),
    ("cdi", Need::Any),
    ("channelf", Need::Any),
    ("colecovision", Need::Any),
    ("fds", Need::Any),
    ("intellivision", Need::All),
    ("megacd", Need::Any),
    ("naomi", Need::Any),
    ("neogeocd", Need::Any),
    ("odyssey2", Need::Any),
    ("pcenginecd", Need::Any),
    ("pcfx", Need::Any),
    ("ps2", Need::Any),
    ("saturn", Need::Any),
    ("segacd", Need::Any),
    ("x68000", Need::All),
];

/// Files bigger than this are taken as right when there: a PS3's firmware is hundreds of MB.
const HASHED_UP_TO: u64 = 64 << 20;

#[derive(Debug, Clone, Deserialize)]
struct Listed {
    #[serde(rename = "biosFiles", default)]
    files: Vec<ListedFile>,
}

#[derive(Debug, Clone, Deserialize)]
struct ListedFile {
    #[serde(default)]
    md5: String,
    file: String,
    #[serde(rename = "zippedFile")]
    zipped: Option<String>,
}

/// Batocera's list of BIOS files, by its system names.
#[derive(Debug, Clone, Default)]
pub struct Table(HashMap<String, Listed>);

impl Table {
    /// Read from Batocera's script; `None` where there is none (not Batocera) or it cannot be
    /// read (the reason is logged).
    pub fn load(script: &Path) -> Option<Self> {
        if !script.is_file() {
            return None;
        }
        let out = host::command("python3")
            .arg("-c")
            .arg(DUMP)
            .arg(script)
            .output();
        match out {
            Ok(out) if out.status.success() => {
                match Self::parse(&String::from_utf8_lossy(&out.stdout)) {
                    Ok(table) => Some(table),
                    Err(e) => {
                        eprintln!("forma: BIOS list from {}: {e}", script.display());
                        None
                    }
                }
            }
            Ok(out) => {
                eprintln!(
                    "forma: BIOS list from {}: {}",
                    script.display(),
                    String::from_utf8_lossy(&out.stderr).trim()
                );
                None
            }
            Err(e) => {
                eprintln!("forma: BIOS list from {}: {e}", script.display());
                None
            }
        }
    }

    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json).map(Self)
    }

    fn listed(&self, system: &str) -> Option<&Listed> {
        self.0.get(system).or_else(|| {
            LISTED_AS
                .iter()
                .find(|(ours, _)| *ours == system)
                .and_then(|(_, theirs)| self.0.get(*theirs))
        })
    }

    /// `system`'s BIOS files and how they are, under `userdata`; `None` when it lists none.
    pub fn check(&self, system: &str, userdata: &Path) -> Option<SystemBios> {
        let listed = self.listed(system)?;
        let mut files: Vec<BiosFile> = Vec::new();
        for f in &listed.files {
            let name = match &f.zipped {
                Some(inner) => format!("{} ({inner})", f.file),
                None => f.file.clone(),
            };
            // Listed twice (the same file for two cores): once is enough.
            if files.iter().any(|b| b.file == name) {
                continue;
            }
            let state = state_of(&userdata.join(&f.file), f.zipped.as_deref(), &f.md5);
            files.push(BiosFile {
                file: name,
                md5: f.md5.clone(),
                state,
            });
        }
        if files.is_empty() {
            return None;
        }
        let need = NEEDED.iter().find(|(s, _)| *s == system).map(|(_, n)| *n);
        let ok = |f: &BiosFile| f.state == BiosState::Ok;
        let missing = match need {
            Some(Need::Any) => !files.iter().any(ok),
            Some(Need::All) => !files.iter().all(ok),
            None => false,
        };
        Some(SystemBios {
            system: system.to_owned(),
            needed: need.is_some(),
            missing,
            files,
        })
    }
}

/// One listed BIOS file: its path under userdata (`bios/scph5501.bin`; one inside a zip as
/// `bios/x.zip (inner.rom)`), the MD5 it should have (empty: any), and how it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BiosFile {
    pub file: String,
    pub md5: String,
    pub state: BiosState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BiosState {
    Ok,
    Missing,
    /// There, but not the file listed (another version, or damaged).
    Wrong,
}

/// A system's BIOS files. `needed`: its games do not start without one; `missing`: so it is
/// now (none of its alternatives there and right, or one of those it needs all of not).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemBios {
    pub system: String,
    pub needed: bool,
    pub missing: bool,
    pub files: Vec<BiosFile>,
}

fn state_of(path: &Path, zipped: Option<&str>, md5: &str) -> BiosState {
    let Ok(meta) = std::fs::metadata(path) else {
        return BiosState::Missing;
    };
    if !meta.is_file() {
        return BiosState::Missing;
    }
    let digest = match zipped {
        None if md5.is_empty() || meta.len() > HASHED_UP_TO => return BiosState::Ok,
        None => std::fs::File::open(path).ok().and_then(|f| md5_of(f).ok()),
        Some(inner) => match inside_zip(path, inner) {
            Inside::Missing => return BiosState::Missing,
            Inside::There if md5.is_empty() => return BiosState::Ok,
            Inside::There => zip_entry_md5(path, inner),
        },
    };
    match digest {
        Some(d) if d.eq_ignore_ascii_case(md5) => BiosState::Ok,
        _ => BiosState::Wrong,
    }
}

enum Inside {
    There,
    Missing,
}

fn inside_zip(path: &Path, inner: &str) -> Inside {
    let found = std::fs::File::open(path)
        .ok()
        .and_then(|f| zip::ZipArchive::new(f).ok())
        .is_some_and(|z| entry_named(&z, inner).is_some());
    if found {
        Inside::There
    } else {
        Inside::Missing
    }
}

fn zip_entry_md5(path: &Path, inner: &str) -> Option<String> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
    let name = entry_named(&archive, inner)?;
    let entry = archive.by_name(&name).ok()?;
    md5_of(entry).ok()
}

/// The name of the entry `inner` in `archive`, whatever its case.
fn entry_named<R: Read + std::io::Seek>(
    archive: &zip::ZipArchive<R>,
    inner: &str,
) -> Option<String> {
    archive
        .file_names()
        .filter_map(Result::ok)
        .find(|n| n.eq_ignore_ascii_case(inner))
        .map(|n| n.into_owned())
}

fn md5_of(mut from: impl Read) -> std::io::Result<String> {
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = r#"{
        "segacd": {"name": "Sega CD", "biosFiles": [
            {"md5": "900150983cd24fb0d6963f7d28e17f72", "file": "bios/bios_CD_E.bin"},
            {"md5": "11111111111111111111111111111111", "file": "bios/bios_CD_U.bin"}]},
        "pcengine": {"name": "PC Engine", "biosFiles": [
            {"md5": "", "file": "bios/syscard3.pce"}]},
        "intellivision": {"name": "Intellivision", "biosFiles": [
            {"md5": "900150983cd24fb0d6963f7d28e17f72", "file": "bios/exec.bin"},
            {"md5": "900150983cd24fb0d6963f7d28e17f72", "file": "bios/grom.bin"}]},
        "cdi": {"name": "CD-i", "biosFiles": [
            {"md5": "", "file": "bios/cdimono1.zip"},
            {"md5": "900150983cd24fb0d6963f7d28e17f72", "file": "bios/cdimono1.zip", "zippedFile": "cdi200.rom"},
            {"md5": "900150983cd24fb0d6963f7d28e17f72", "file": "bios/cdimono1.zip", "zippedFile": "cdi220.rom"}]}
    }"#;

    fn userdata() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("bios")).unwrap();
        dir
    }

    #[test]
    fn one_region_is_enough_for_a_mega_cd_listed_as_sega_cd() {
        let table = Table::parse(TABLE).unwrap();
        let dir = userdata();
        let none = table.check("megacd", dir.path()).unwrap();
        assert!(none.needed && none.missing);
        // "abc", whose MD5 the list has for the European BIOS.
        std::fs::write(dir.path().join("bios/bios_CD_E.bin"), "abc").unwrap();
        std::fs::write(dir.path().join("bios/bios_CD_U.bin"), "not it").unwrap();
        let one = table.check("megacd", dir.path()).unwrap();
        assert!(!one.missing);
        assert_eq!(
            one.files.iter().map(|f| f.state).collect::<Vec<_>>(),
            [BiosState::Ok, BiosState::Wrong]
        );
    }

    #[test]
    fn a_pc_engine_plays_cartridges_without_its_cd_card() {
        let table = Table::parse(TABLE).unwrap();
        let dir = userdata();
        let hucard = table.check("pcengine", dir.path()).unwrap();
        assert!(!hucard.needed && !hucard.missing);
        assert_eq!(hucard.files[0].state, BiosState::Missing);
        assert!(table.check("pcenginecd", dir.path()).unwrap().missing);
        assert_eq!(table.check("snes", dir.path()), None);
    }

    #[test]
    fn some_systems_need_every_file() {
        let table = Table::parse(TABLE).unwrap();
        let dir = userdata();
        std::fs::write(dir.path().join("bios/exec.bin"), "abc").unwrap();
        assert!(table.check("intellivision", dir.path()).unwrap().missing);
        std::fs::write(dir.path().join("bios/grom.bin"), "abc").unwrap();
        assert!(!table.check("intellivision", dir.path()).unwrap().missing);
    }

    #[test]
    fn files_inside_a_zip_are_checked_inside_it() {
        let table = Table::parse(TABLE).unwrap();
        let dir = userdata();
        let zip_path = dir.path().join("bios/cdimono1.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        zip.start_file("CDI200.ROM", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, b"abc").unwrap();
        zip.finish().unwrap();
        let cdi = table.check("cdi", dir.path()).unwrap();
        let states: Vec<_> = cdi
            .files
            .iter()
            .map(|f| (f.file.as_str(), f.state))
            .collect();
        assert_eq!(
            states,
            [
                ("bios/cdimono1.zip", BiosState::Ok),
                ("bios/cdimono1.zip (cdi200.rom)", BiosState::Ok),
                ("bios/cdimono1.zip (cdi220.rom)", BiosState::Missing)
            ]
        );
        assert!(!cdi.missing);
    }
}
