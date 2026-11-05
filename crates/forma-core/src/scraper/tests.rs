use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::*;
use crate::library::Game;
use crate::net::{Http, HttpError};
use crate::paths::Paths;
use crate::systems::System;

pub(crate) const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake";
const MP4: &[u8] = b"\0\0\0\x18ftypmp42rest";

/// Serves bytes by exact URL; anything else is a 404. Remembers what was asked.
pub(crate) struct Serve {
    pub files: BTreeMap<String, Vec<u8>>,
    pub asked: Mutex<Vec<String>>,
}

impl Serve {
    pub fn new(files: &[(&str, &[u8])]) -> Self {
        Self {
            files: files
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.to_vec()))
                .collect(),
            asked: Mutex::new(Vec::new()),
        }
    }
}

impl Http for Serve {
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        self.asked.lock().unwrap().push(url.to_owned());
        self.files.get(url).cloned().ok_or(HttpError {
            status: Some(404),
            message: "not found".into(),
        })
    }
}

/// A source that answers from a list, or fails as told.
struct Fake {
    id: &'static str,
    gives: Vec<MediaKind>,
    answer: Result<Option<Found>, LookupError>,
    asked: Mutex<u32>,
}

impl Fake {
    fn new(
        id: &'static str,
        gives: &[MediaKind],
        answer: Result<Option<Found>, LookupError>,
    ) -> Box<Self> {
        Box::new(Self {
            id,
            gives: gives.to_vec(),
            answer,
            asked: Mutex::new(0),
        })
    }
}

impl Source for Fake {
    fn id(&self) -> &str {
        self.id
    }
    fn name(&self) -> &str {
        self.id
    }
    fn about(&self) -> &str {
        ""
    }
    fn gives(&self) -> &[MediaKind] {
        &self.gives
    }
    fn lookup(
        &self,
        _: &dyn Http,
        _: &Credentials,
        _: &Query<'_>,
    ) -> Result<Option<Found>, LookupError> {
        *self.asked.lock().unwrap() += 1;
        self.answer.clone()
    }
}

fn system(dir: &Path) -> System {
    System {
        name: "snes".into(),
        fullname: "Super Nintendo".into(),
        manufacturer: None,
        release_year: None,
        hardware: None,
        rom_path: dir.to_path_buf(),
        extensions: vec![".sfc".into()],
        command: None,
        platforms: vec!["snes".into()],
        theme: None,
        emulators: Vec::new(),
    }
}

fn game(path: &str, name: &str) -> Game {
    Game {
        id: 1,
        system: "snes".into(),
        path: path.into(),
        name: name.into(),
        description: None,
        developer: None,
        publisher: None,
        genre: None,
        players: None,
        release_date: None,
        rating: None,
        favorite: false,
        hidden: false,
        play_count: 0,
        last_played: None,
        game_time: 0,
        ra_game_id: None,
        region: None,
        lang: None,
        media: BTreeMap::new(),
    }
}

fn config(ids: &[&str], media: &[MediaKind]) -> Config {
    Config {
        sources: ids
            .iter()
            .map(|id| SourceChoice {
                id: (*id).into(),
                enabled: true,
            })
            .collect(),
        media: media.to_vec(),
        metadata: true,
        replace: false,
    }
}

fn paths(dir: &Path) -> Paths {
    Paths {
        userdata: dir.to_path_buf(),
        systems_cfg: dir.join("es_systems.cfg"),
    }
}

fn meta(description: &str, developer: Option<&str>) -> Metadata {
    Metadata {
        name: Some("Super Metroid".into()),
        description: Some(description.into()),
        developer: developer.map(Into::into),
        release_date: Some("1994-03-19".into()),
        rating: Some(0.9),
        ..Metadata::default()
    }
}

#[test]
fn asks_sources_in_order_falls_back_per_kind_and_writes_like_es() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Super Metroid (USA).sfc"), b"rom").unwrap();
    let http = Serve::new(&[
        ("https://b/box.png", PNG),
        ("https://b/shot.png", PNG),
        ("https://b/video.mp4", MP4),
        // Not a picture: an error page.
        ("https://a/shot.png", b"<html>nope</html>"),
    ]);
    let sources: Vec<Box<dyn Source>> = vec![
        Fake::new(
            "a",
            &[MediaKind::Thumbnail, MediaKind::Image],
            Ok(Some(Found {
                metadata: meta("From A", None),
                media: vec![
                    (MediaKind::Thumbnail, "https://a/missing.png".into()),
                    (MediaKind::Image, "https://a/shot.png".into()),
                ],
            })),
        ),
        Fake::new(
            "b",
            &[MediaKind::Thumbnail, MediaKind::Image, MediaKind::Video],
            Ok(Some(Found {
                metadata: meta("From B", Some("Nintendo R&D1")),
                media: vec![
                    (MediaKind::Thumbnail, "https://b/box.png".into()),
                    (MediaKind::Image, "https://b/shot.png".into()),
                    (MediaKind::Video, "https://b/video.mp4".into()),
                ],
            })),
        ),
    ];
    let cfg = config(
        &["a", "b"],
        &[MediaKind::Thumbnail, MediaKind::Image, MediaKind::Video],
    );
    let out = scrape_game(
        &http,
        &paths(dir.path()),
        &sources,
        &cfg,
        &BTreeMap::new(),
        &game("Super Metroid (USA).sfc", "Super Metroid (USA)"),
        &system(dir.path()),
        &mut HashSet::new(),
    )
    .unwrap();
    assert_eq!(out.sources, ["a", "b"]);
    assert_eq!(
        out.media,
        [MediaKind::Thumbnail, MediaKind::Image, MediaKind::Video]
    );
    assert!(out.metadata);
    assert!(
        dir.path()
            .join("images/Super Metroid (USA)-thumb.png")
            .exists()
    );
    assert!(
        dir.path()
            .join("images/Super Metroid (USA)-image.png")
            .exists()
    );
    assert!(
        dir.path()
            .join("videos/Super Metroid (USA)-video.mp4")
            .exists()
    );
    let list = std::fs::read_to_string(dir.path().join("gamelist.xml")).unwrap();
    // A's text first; what A lacked, from B; the name replaces the file's.
    assert!(list.contains("<desc>From A</desc>"), "{list}");
    assert!(
        list.contains("<developer>Nintendo R&amp;D1</developer>"),
        "{list}"
    );
    assert!(list.contains("<name>Super Metroid</name>"), "{list}");
    assert!(
        list.contains("<releasedate>19940319T000000</releasedate>"),
        "{list}"
    );
    assert!(list.contains("<rating>0.90</rating>"), "{list}");
    assert!(
        list.contains("<thumbnail>./images/Super Metroid (USA)-thumb.png</thumbnail>"),
        "{list}"
    );
    assert!(
        list.contains("<video>./videos/Super Metroid (USA)-video.mp4</video>"),
        "{list}"
    );
}

#[test]
fn fills_only_what_is_missing_and_stops_asking_once_done() {
    let dir = tempfile::tempdir().unwrap();
    let box_png = dir.path().join("images/Game-thumb.png");
    std::fs::create_dir_all(box_png.parent().unwrap()).unwrap();
    std::fs::write(&box_png, PNG).unwrap();
    let http = Serve::new(&[("https://a/box.png", PNG)]);
    let first = Fake::new(
        "a",
        &[MediaKind::Thumbnail],
        Ok(Some(Found {
            metadata: Metadata {
                description: Some("New".into()),
                ..Metadata::default()
            },
            media: vec![(MediaKind::Thumbnail, "https://a/box.png".into())],
        })),
    );
    let second = Fake::new("b", &[MediaKind::Thumbnail], Ok(None));
    let sources: Vec<Box<dyn Source>> = vec![first, second];
    let mut g = game("Game.sfc", "Game");
    g.name = "A Game".into();
    g.description = Some("Mine".into());
    g.media
        .insert("thumbnail".into(), box_png.to_string_lossy().into_owned());
    let mut cfg = config(&["a", "b"], &[MediaKind::Thumbnail]);
    cfg.metadata = false;

    // It has its box and metadata is off: nothing to do, nobody asked.
    let out = scrape_game(
        &http,
        &paths(dir.path()),
        &sources,
        &cfg,
        &BTreeMap::new(),
        &g,
        &system(dir.path()),
        &mut HashSet::new(),
    )
    .unwrap();
    assert!(out.skipped);
    assert!(http.asked.lock().unwrap().is_empty());

    // Replacing: A gives the box, so B is not asked.
    cfg.replace = true;
    cfg.metadata = true;
    let out = scrape_game(
        &http,
        &paths(dir.path()),
        &sources,
        &cfg,
        &BTreeMap::new(),
        &g,
        &system(dir.path()),
        &mut HashSet::new(),
    )
    .unwrap();
    assert_eq!(out.sources, ["a"]);
    let list = std::fs::read_to_string(dir.path().join("gamelist.xml")).unwrap();
    assert!(list.contains("<desc>New</desc>"), "{list}");
}

#[test]
fn a_source_that_refuses_is_dropped_for_the_run_and_one_not_ready_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let http = Serve::new(&[]);
    let sources: Vec<Box<dyn Source>> = vec![
        Fake::new(
            "quota",
            &[MediaKind::Image],
            Err(LookupError::Quota("too many".into())),
        ),
        sources::builtin().remove(1), // TheGamesDB: needs a key, none given.
    ];
    let cfg = config(&["quota", "thegamesdb"], &[MediaKind::Image]);
    let mut out_of_service = HashSet::new();
    let out = scrape_game(
        &http,
        &paths(dir.path()),
        &sources,
        &cfg,
        &BTreeMap::new(),
        &game("G.sfc", "G"),
        &system(dir.path()),
        &mut out_of_service,
    )
    .unwrap();
    assert!(!out.found());
    assert_eq!(out.problems, ["quota: too many"]);
    assert!(out_of_service.contains("quota"));
    assert!(
        http.asked.lock().unwrap().is_empty(),
        "TheGamesDB without a key is not asked"
    );
    assert!(!dir.path().join("gamelist.xml").exists());
}

#[test]
fn config_lists_every_source_keeping_the_users_order() {
    let all = builtin();
    let stored = r#"{"sources":[{"id":"libretro","enabled":true},{"id":"gone","enabled":true},{"id":"igdb","enabled":false},{"id":"libretro","enabled":false}],"media":["video","thumbnail","video"]}"#;
    let c = Config::load(Some(stored), &all);
    let ids: Vec<&str> = c.sources.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "libretro",
            "igdb",
            "screenscraper",
            "thegamesdb",
            "mobygames",
            "steamgriddb",
            "arcadedb"
        ]
    );
    assert!(!c.sources[1].enabled);
    assert_eq!(c.media, [MediaKind::Thumbnail, MediaKind::Video]);
    assert!(c.metadata && !c.replace);
    assert_eq!(Config::load(None, &all).sources.len(), 7);
    assert_eq!(
        Config::load(Some("not json"), &all).media,
        Config::default().media
    );
}

#[test]
fn es_file_names_and_folders() {
    assert_eq!(MediaKind::Thumbnail.suffix(), "thumb");
    assert_eq!(MediaKind::Boxart.suffix(), "box");
    assert_eq!(MediaKind::Video.folder(), "videos");
    assert_eq!(MediaKind::Manual.folder(), "manuals");
    let _ = PathBuf::new();
}

/// Against the real services; `cargo test -- --ignored live`.
#[test]
#[ignore = "network"]
fn live_keyless_sources_save_real_media() {
    let dir = tempfile::tempdir().unwrap();
    let http = crate::net::UreqHttp::default();
    let all = builtin();
    let cfg = config(
        &["arcadedb", "libretro"],
        &[
            MediaKind::Thumbnail,
            MediaKind::Image,
            MediaKind::Titleshot,
            MediaKind::Marquee,
            MediaKind::Video,
        ],
    );
    let mut snes = system(dir.path());
    let out = scrape_game(
        &http,
        &paths(dir.path()),
        &all,
        &cfg,
        &BTreeMap::new(),
        &game(
            "Super Metroid (Japan, USA) (En,Ja).sfc",
            "Super Metroid (Japan, USA) (En,Ja)",
        ),
        &snes,
        &mut HashSet::new(),
    )
    .unwrap();
    eprintln!("snes: {out:?}");
    assert_eq!(out.sources, ["libretro"]);
    assert!(out.media.contains(&MediaKind::Thumbnail));
    snes.name = "mame".into();
    snes.platforms = vec!["arcade".into()];
    let out = scrape_game(
        &http,
        &paths(dir.path()),
        &all,
        &cfg,
        &BTreeMap::new(),
        &game("sf2.zip", "sf2"),
        &snes,
        &mut HashSet::new(),
    )
    .unwrap();
    eprintln!("mame: {out:?}");
    assert!(out.sources.contains(&"arcadedb".to_owned()) && out.metadata);
    eprintln!(
        "{}",
        std::fs::read_to_string(dir.path().join("gamelist.xml")).unwrap()
    );
    for e in std::fs::read_dir(dir.path().join("images")).unwrap() {
        let e = e.unwrap();
        eprintln!(
            "{} {}",
            e.file_name().to_string_lossy(),
            e.metadata().unwrap().len()
        );
    }
}
