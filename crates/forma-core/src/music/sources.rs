//! What there is to play: the music folder's sub-folders and playlist files, its loose tracks,
//! the web radios in `radios.m3u`, and Batocera's own music when the folder has none.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};

const AUDIO: &[&str] = &["mp3", "ogg", "oga", "opus", "flac", "wav", "m4a", "aac"];
const PLAYLISTS: &[&str] = &["m3u", "m3u8", "pls"];
/// The music folder's web radios, one per entry, as an extended M3U.
pub const RADIOS_FILE: &str = "radios.m3u";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    /// A folder of tracks (sub-folders included), or the music folder's own loose tracks.
    Folder,
    /// An `.m3u` or `.pls` file.
    Playlist,
    Radio,
}

/// Something the player can play. `id` is stable across scans, so a theme (and Forma, for
/// "resume at boot") can remember it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub kind: SourceKind,
    pub name: String,
    /// Tracks in a folder or playlist; 0 for a radio.
    pub tracks: usize,
}

/// A source, resolved to what mpv loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Playable {
    /// Files, in order.
    Tracks(Vec<PathBuf>),
    /// A playlist file mpv reads itself.
    List(PathBuf),
    Url(String),
}

pub struct Library {
    /// `/userdata/music`.
    pub music: PathBuf,
    /// Batocera's own music, played when the music folder has none.
    pub fallback: PathBuf,
    /// Web radios from `music` add-ons, by add-on id: listed after the user's own, with ids
    /// `addon:<id>:<url>`. Forma fills it from the add-ons (see `crate::addons`).
    pub addon_radios: std::sync::RwLock<Vec<(String, Radio)>>,
}

impl Library {
    pub fn sources(&self) -> Vec<Source> {
        let mut out = Vec::new();
        let loose = tracks_in(&self.music, false);
        if !loose.is_empty() {
            out.push(Source {
                id: "folder:".into(),
                kind: SourceKind::Folder,
                name: "Music".into(),
                tracks: loose.len(),
            });
        }
        for dir in sorted_entries(&self.music)
            .into_iter()
            .filter(|p| p.is_dir())
        {
            let tracks = tracks_in(&dir, true).len();
            if tracks > 0 {
                out.push(Source {
                    id: format!("folder:{}", name_of(&dir)),
                    kind: SourceKind::Folder,
                    name: name_of(&dir),
                    tracks,
                });
            }
        }
        for file in sorted_entries(&self.music)
            .into_iter()
            .filter(|p| has_ext(p, PLAYLISTS))
        {
            if file.file_name().is_some_and(|n| n == RADIOS_FILE) {
                continue;
            }
            out.push(Source {
                id: format!("playlist:{}", name_of(&file)),
                kind: SourceKind::Playlist,
                name: stem_of(&file),
                tracks: playlist_len(&file),
            });
        }
        if out.is_empty() && !tracks_in(&self.fallback, true).is_empty() {
            out.push(Source {
                id: "batocera:".into(),
                kind: SourceKind::Folder,
                name: "Batocera".into(),
                tracks: tracks_in(&self.fallback, true).len(),
            });
        }
        for radio in self.radios() {
            out.push(Source {
                id: format!("radio:{}", radio.url),
                kind: SourceKind::Radio,
                name: radio.name,
                tracks: 0,
            });
        }
        for (addon, radio) in self.addon_radios() {
            out.push(Source {
                id: format!("addon:{addon}:{}", radio.url),
                kind: SourceKind::Radio,
                name: radio.name,
                tracks: 0,
            });
        }
        out
    }

    fn addon_radios(&self) -> Vec<(String, Radio)> {
        self.addon_radios
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Replaces the add-ons' radios.
    pub fn set_addon_radios(&self, radios: Vec<(String, Radio)>) {
        *self
            .addon_radios
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = radios;
    }

    /// What to load for `id`, or why not.
    pub fn resolve(&self, id: &str) -> Result<Playable> {
        let unknown = || Error::Invalid(format!("nothing to play called `{id}`"));
        let (kind, rest) = id.split_once(':').ok_or_else(unknown)?;
        // A folder or playlist is a name from a scan of the music folder: never a path out of it.
        let local = matches!(kind, "folder" | "playlist");
        if local && (rest.contains('/') || rest.contains('\\') || rest == "..") {
            return Err(unknown());
        }
        match kind {
            "folder" if rest.is_empty() => {
                non_empty(tracks_in(&self.music, false)).ok_or_else(unknown)
            }
            "folder" => non_empty(tracks_in(&self.music.join(rest), true)).ok_or_else(unknown),
            "batocera" => non_empty(tracks_in(&self.fallback, true)).ok_or_else(unknown),
            "playlist" => {
                let file = self.music.join(rest);
                (file.is_file() && has_ext(&file, PLAYLISTS))
                    .then_some(Playable::List(file))
                    .ok_or_else(unknown)
            }
            "radio" => self
                .radios()
                .into_iter()
                .find(|r| r.url == rest)
                .map(|r| Playable::Url(r.url))
                .ok_or_else(unknown),
            "addon" => {
                let (addon, url) = rest.split_once(':').ok_or_else(unknown)?;
                self.addon_radios()
                    .into_iter()
                    .find(|(a, r)| a == addon && r.url == url)
                    .map(|(_, r)| Playable::Url(r.url))
                    .ok_or_else(unknown)
            }
            _ => Err(unknown()),
        }
    }

    pub fn radios(&self) -> Vec<Radio> {
        parse_radios(&std::fs::read_to_string(self.music.join(RADIOS_FILE)).unwrap_or_default())
    }

    /// Adds a web radio to `radios.m3u` (or renames one with the same URL).
    pub fn add_radio(&self, name: &str, url: &str) -> Result<()> {
        let name = name.trim();
        let url = url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://"))
            || url.contains(char::is_whitespace)
        {
            return Err(Error::Invalid(format!("{url:?} is not a web address")));
        }
        if name.is_empty() || name.contains(['\n', '\r']) {
            return Err(Error::Invalid("a radio needs a name, on one line".into()));
        }
        let mut radios = self.radios();
        radios.retain(|r| r.url != url);
        radios.push(Radio {
            name: name.to_owned(),
            url: url.to_owned(),
        });
        self.write_radios(&radios)
    }

    pub fn remove_radio(&self, url: &str) -> Result<()> {
        let mut radios = self.radios();
        let before = radios.len();
        radios.retain(|r| r.url != url);
        if radios.len() == before {
            return Err(Error::Invalid(format!("no radio at {url}")));
        }
        self.write_radios(&radios)
    }

    fn write_radios(&self, radios: &[Radio]) -> Result<()> {
        let mut text = String::from("#EXTM3U\n");
        for r in radios {
            text.push_str(&format!("#EXTINF:-1,{}\n{}\n", r.name, r.url));
        }
        let file = self.music.join(RADIOS_FILE);
        std::fs::create_dir_all(&self.music).map_err(|e| Error::io(&self.music, e))?;
        std::fs::write(&file, text).map_err(|e| Error::io(&file, e))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Radio {
    pub name: String,
    pub url: String,
}

/// An extended M3U of web addresses: `#EXTINF:-1,Name` before each, or the address as the name.
fn parse_radios(text: &str) -> Vec<Radio> {
    let mut out = Vec::new();
    let mut name: Option<String> = None;
    for line in text.lines().map(str::trim) {
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            name = info.split_once(',').map(|(_, n)| n.trim().to_owned());
        } else if line.starts_with("http://") || line.starts_with("https://") {
            out.push(Radio {
                name: name
                    .take()
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| line.to_owned()),
                url: line.to_owned(),
            });
        } else if !line.starts_with('#') && !line.is_empty() {
            name = None;
        }
    }
    out
}

fn non_empty(tracks: Vec<PathBuf>) -> Option<Playable> {
    (!tracks.is_empty()).then_some(Playable::Tracks(tracks))
}

fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Entries of a directory, hidden ones left out, by name.
fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| !name_of(p).starts_with('.'))
        .collect();
    entries.sort_by_key(|p| name_of(p).to_lowercase());
    entries
}

/// Audio files in `dir`, by name; with `deep`, in its sub-folders too.
fn tracks_in(dir: &Path, deep: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in sorted_entries(dir) {
        if path.is_dir() {
            if deep {
                out.extend(tracks_in(&path, true));
            }
        } else if has_ext(&path, AUDIO) {
            out.push(path);
        }
    }
    out
}

/// Entries in a playlist file: the lines that are not comments (M3U), or `FileN=` (PLS).
fn playlist_len(file: &Path) -> usize {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    if has_ext(file, &["pls"]) {
        text.lines()
            .filter(|l| l.trim_start().to_lowercase().starts_with("file"))
            .count()
    } else {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let music = dir.path().join("music");
        let touch = |p: &str| {
            let path = music.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        };
        touch("b-side.mp3");
        touch("A side.ogg");
        touch("readme.txt");
        touch("Chiptune/Sonic.ogg");
        touch("Chiptune/More/Mario.flac");
        touch("Empty/notes.txt");
        touch(".hidden/x.mp3");
        std::fs::write(
            music.join("Road trip.m3u"),
            "#EXTM3U\n#EXTINF:1,a\na.mp3\nb.mp3\n",
        )
        .unwrap();
        std::fs::write(
            music.join(RADIOS_FILE),
            "#EXTM3U\n#EXTINF:-1,Groove Salad\nhttps://ice1.somafm.com/groovesalad-128-mp3\nhttp://radio.example/live\n",
        )
        .unwrap();
        let fallback = dir.path().join("batocera-music");
        let lib = Library {
            music,
            fallback,
            addon_radios: Default::default(),
        };
        (dir, lib)
    }

    #[test]
    fn lists_loose_tracks_folders_playlists_and_radios() {
        let (_dir, lib) = library();
        let sources: Vec<(SourceKind, String, String, usize)> = lib
            .sources()
            .into_iter()
            .map(|s| (s.kind, s.id, s.name, s.tracks))
            .collect();
        assert_eq!(
            sources,
            [
                (SourceKind::Folder, "folder:".into(), "Music".into(), 2),
                (
                    SourceKind::Folder,
                    "folder:Chiptune".into(),
                    "Chiptune".into(),
                    2
                ),
                (
                    SourceKind::Playlist,
                    "playlist:Road trip.m3u".into(),
                    "Road trip".into(),
                    2
                ),
                (
                    SourceKind::Radio,
                    "radio:https://ice1.somafm.com/groovesalad-128-mp3".into(),
                    "Groove Salad".into(),
                    0
                ),
                (
                    SourceKind::Radio,
                    "radio:http://radio.example/live".into(),
                    "http://radio.example/live".into(),
                    0
                ),
            ]
        );
    }

    #[test]
    fn resolves_ids_and_refuses_paths_out_of_the_folder() {
        let (_dir, lib) = library();
        let Playable::Tracks(loose) = lib.resolve("folder:").unwrap() else {
            panic!()
        };
        assert_eq!(
            loose.iter().map(|p| name_of(p)).collect::<Vec<_>>(),
            ["A side.ogg", "b-side.mp3"]
        );
        let Playable::Tracks(chip) = lib.resolve("folder:Chiptune").unwrap() else {
            panic!()
        };
        assert_eq!(chip.len(), 2, "sub-folders included");
        assert_eq!(
            lib.resolve("playlist:Road trip.m3u").unwrap(),
            Playable::List(lib.music.join("Road trip.m3u"))
        );
        assert_eq!(
            lib.resolve("radio:http://radio.example/live").unwrap(),
            Playable::Url("http://radio.example/live".into())
        );
        for bad in [
            "folder:..",
            "folder:../etc",
            "playlist:../x.m3u",
            "radio:http://nope",
            "folder:Empty",
            "x",
        ] {
            assert!(lib.resolve(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn falls_back_to_batocera_music_when_the_folder_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let fallback = dir.path().join("batocera-music");
        std::fs::create_dir_all(&fallback).unwrap();
        std::fs::write(fallback.join("Bolt Action - Give Me.ogg"), "").unwrap();
        let lib = Library {
            music: dir.path().join("music"),
            fallback,
            addon_radios: Default::default(),
        };
        let sources = lib.sources();
        assert_eq!(sources.len(), 1);
        assert_eq!(
            (sources[0].id.as_str(), sources[0].tracks),
            ("batocera:", 1)
        );
        assert!(matches!(lib.resolve("batocera:").unwrap(), Playable::Tracks(t) if t.len() == 1));
    }

    #[test]
    fn adds_renames_and_removes_radios_in_radios_m3u() {
        let (_dir, lib) = library();
        lib.add_radio(" Lofi ", "https://lofi.example/stream")
            .unwrap();
        lib.add_radio(
            "Groove Salad (SomaFM)",
            "https://ice1.somafm.com/groovesalad-128-mp3",
        )
        .unwrap();
        lib.remove_radio("http://radio.example/live").unwrap();
        assert_eq!(
            lib.radios(),
            [
                Radio {
                    name: "Lofi".into(),
                    url: "https://lofi.example/stream".into()
                },
                Radio {
                    name: "Groove Salad (SomaFM)".into(),
                    url: "https://ice1.somafm.com/groovesalad-128-mp3".into()
                },
            ]
        );
        assert!(
            std::fs::read_to_string(lib.music.join(RADIOS_FILE))
                .unwrap()
                .starts_with("#EXTM3U\n#EXTINF:-1,Lofi\n")
        );
        assert!(lib.add_radio("x", "ftp://x").is_err());
        assert!(lib.add_radio("", "https://x").is_err());
        assert!(lib.add_radio("two\nlines", "https://x").is_err());
        assert!(lib.remove_radio("https://not-there").is_err());
    }
}
