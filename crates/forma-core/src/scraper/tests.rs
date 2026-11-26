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

pub(super) fn system(dir: &Path) -> System {
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

pub(super) fn game(path: &str, name: &str) -> Game {
    Game {
        id: 1,
        system: "snes".into(),
        path: path.into(),
        name: name.into(),
        given_name: name.into(),
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
        age_rating: None,
        iconic: false,
        media: BTreeMap::new(),
        dump: None,
        discs: Vec::new(),
    }
}

/// A config asking `ids` in order, for a theme that draws `media` (none said: the default).
fn config(ids: &[&str], media: &[MediaKind]) -> Config {
    Config {
        sources: ids
            .iter()
            .map(|id| SourceChoice {
                id: (*id).into(),
                enabled: true,
            })
            .collect(),
        needs: super::Needs {
            shows: media.to_vec(),
            ..Default::default()
        },
        ..Config::default()
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
                notes: Default::default(),
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
                notes: Default::default(),
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
            notes: Default::default(),
        })),
    );
    let second = Fake::new("b", &[MediaKind::Thumbnail], Ok(None));
    let sources: Vec<Box<dyn Source>> = vec![first, second];
    let mut g = game("Game.sfc", "Game");
    g.name = "A Game".into();
    g.given_name = g.name.clone();
    g.description = Some("Mine".into());
    g.media
        .insert("thumbnail".into(), box_png.to_string_lossy().into_owned());
    let mut cfg = config(&["a", "b"], &[MediaKind::Thumbnail]);
    cfg.skip_scraped = true;

    // It has been scraped, and those are skipped: nothing to do, nobody asked.
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
    cfg.skip_scraped = false;

    // It has its box and every detail: nothing to do either.
    g.developer = Some("Me".into());
    g.publisher = Some("Me".into());
    g.genre = Some("Action".into());
    g.players = Some("1".into());
    g.release_date = Some("1994".into());
    g.rating = Some(0.5);
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
    let stored = r#"{"sources":[{"id":"libretro","enabled":true},{"id":"gone","enabled":true},{"id":"igdb","enabled":false},{"id":"libretro","enabled":false}],"regions":["Europe","Mars","USA"],"ownRegion":false}"#;
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
            "hfsdb",
            "steamgriddb",
            "arcadedb",
            "launchbox"
        ]
    );
    assert!(!c.sources[1].enabled);
    assert_eq!(c.regions, ["Europe", "USA"], "unknown regions left out");
    assert!(!c.own_region && !c.replace && !c.skip_scraped);
    assert_eq!(Config::load(None, &all).sources.len(), 9);
    assert_eq!(
        Config::load(Some("not json"), &all).regions,
        ["World", "USA", "Europe"]
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

#[test]
fn age_ratings_are_kept_short_known_and_in_order() {
    use super::text::age_ratings;
    assert_eq!(
        age_ratings([
            ("pegi", "18"),
            ("ESRB", "M - Mature 17+"),
            ("SS", "16"),
            ("ESRB", "T")
        ]),
        Some("ESRB:M,PEGI:18".into())
    );
    assert_eq!(age_ratings([("ESRB", "RP - Rating Pending")]), None);
    assert_eq!(
        age_ratings([("CLASS_IND", "L"), ("USK", "USK 12")]),
        Some("USK:12,CLASSIND:L".into())
    );
    assert_eq!(age_ratings([]), None);
}

#[test]
fn a_theme_adds_what_it_needs_unless_turned_off() {
    let asked = crate::themes::ThemeScraper {
        media: vec![
            "boxside".into(),
            "thumbnail".into(),
            "hologram".into(),
            "boxside".into(),
        ],
        fields: vec!["ageRating".into(), "aura".into()],
        ..Default::default()
    };
    let needs = super::Needs::from_theme(&asked, None);
    assert_eq!(
        needs.media,
        [MediaKind::Boxside, MediaKind::Thumbnail],
        "unknown ones left out"
    );
    assert!(needs.age_rating);

    let mut cfg = config(&[], &[]);
    cfg.needs = needs;
    // A theme that does not say what it draws: the usual kinds, then what it asks for.
    assert_eq!(
        cfg.all_media(),
        [
            MediaKind::Thumbnail,
            MediaKind::Image,
            MediaKind::Marquee,
            MediaKind::Wheel,
            MediaKind::Video,
            MediaKind::Boxside
        ]
    );
    assert!(cfg.wants_age_rating());
    // One that does: what it draws, but not a backup.
    cfg.needs.shows = vec![MediaKind::Thumbnail, MediaKind::Wheel, MediaKind::Marquee];
    cfg.needs.backups = vec![(MediaKind::Marquee, MediaKind::Wheel)];
    assert_eq!(
        cfg.all_media(),
        [MediaKind::Thumbnail, MediaKind::Wheel, MediaKind::Boxside]
    );
    // Manuals, when the user does not want them downloaded.
    cfg.needs.shows.push(MediaKind::Manual);
    assert!(cfg.all_media().contains(&MediaKind::Manual));
    cfg.manuals = false;
    assert!(!cfg.all_media().contains(&MediaKind::Manual));
    // Still on offer for one game, to choose by hand.
    assert!(cfg.theme_media().contains(&MediaKind::Manual));
}

#[test]
fn a_missing_age_rating_is_scraped_only_for_a_theme_that_shows_it() {
    let dir = tempfile::tempdir().unwrap();
    let http = Serve::new(&[]);
    let found = Found {
        metadata: Metadata {
            age_rating: Some("PEGI:3".into()),
            ..Metadata::default()
        },
        media: Vec::new(),
        notes: Default::default(),
    };
    let sources: Vec<Box<dyn Source>> = vec![Fake::new("a", &[], Ok(Some(found)))];
    let mut g = game("Game.sfc", "A Game");
    g.description = Some("d".into());
    g.developer = Some("d".into());
    g.publisher = Some("p".into());
    g.genre = Some("g".into());
    g.players = Some("1".into());
    g.release_date = Some("1994".into());
    g.rating = Some(0.5);
    // The theme draws only a box, which the game has.
    let thumb = dir.path().join("thumb.png");
    std::fs::write(&thumb, b"png").unwrap();
    g.media
        .insert("thumbnail".into(), thumb.to_string_lossy().into_owned());
    let mut cfg = config(&["a"], &[MediaKind::Thumbnail]);
    let run = |cfg: &Config| {
        scrape_game(
            &http,
            &paths(dir.path()),
            &sources,
            cfg,
            &BTreeMap::new(),
            &g,
            &system(dir.path()),
            &mut HashSet::new(),
        )
        .unwrap()
    };

    assert!(run(&cfg).skipped, "no theme asks for it");
    cfg.needs.age_rating = true;
    let out = run(&cfg);
    assert_eq!(out.sources, ["a"]);
    let list = std::fs::read_to_string(dir.path().join("gamelist.xml")).unwrap();
    assert!(list.contains("<agerating>PEGI:3</agerating>"), "{list}");
}

#[test]
fn a_theme_asks_for_more_in_the_view_in_use() {
    let asked: crate::themes::ThemeScraper = serde_json::from_str(
        r#"{ "media": ["fanart"], "views": { "collector": { "media": ["boxside", "fanart"], "fields": ["ageRating"] } } }"#,
    )
    .unwrap();
    let plain = Needs::from_theme(&asked, Some("immersive"));
    assert_eq!(plain.media, [MediaKind::Fanart]);
    assert!(!plain.age_rating);
    let collector = Needs::from_theme(&asked, Some("collector"));
    assert_eq!(collector.media, [MediaKind::Fanart, MediaKind::Boxside]);
    assert!(collector.age_rating);
}

#[test]
fn offers_come_from_every_source_section_by_section_and_keep_addresses_private() {
    let dir = tempfile::tempdir().unwrap();
    let system = system(dir.path());
    let rom = game("Sonic.md", "Sonic");
    let sources: Vec<Box<dyn Source>> = vec![
        Fake::new(
            "one",
            &[MediaKind::Thumbnail],
            Ok(Some(Found {
                metadata: meta("From one", Some("Sega")),
                media: vec![(
                    MediaKind::Thumbnail,
                    "https://one.test/box.png?password=hunter2".to_owned(),
                )],
                notes: Default::default(),
            })),
        ),
        Fake::new(
            "two",
            &[MediaKind::Thumbnail],
            Ok(Some(Found {
                metadata: meta("From two", None),
                media: vec![(MediaKind::Thumbnail, "https://two.test/box.png".to_owned())],
                notes: Default::default(),
            })),
        ),
    ];
    let config = config(&["one", "two"], &[MediaKind::Thumbnail]);
    let http = Serve::new(&[("https://two.test/box.png", PNG)]);
    let picking = pick::gather(&http, &sources, &config, &BTreeMap::new(), &rom, &system);

    let description = picking
        .offers
        .text
        .iter()
        .find(|s| s.field == "description")
        .unwrap();
    assert_eq!(
        description
            .offers
            .iter()
            .map(|o| (o.source.as_str(), o.value.as_str()))
            .collect::<Vec<_>>(),
        [("one", "From one"), ("two", "From two")]
    );
    let developer = picking
        .offers
        .text
        .iter()
        .find(|s| s.field == "developer")
        .unwrap();
    assert_eq!(developer.offers.len(), 1);
    assert_eq!(picking.offers.media[0].offers.len(), 2);
    assert!(
        !serde_json::to_string(&picking.offers)
            .unwrap()
            .contains("hunter2")
    );

    // The user's picks: two's picture and description; nothing else is touched.
    let picked = BTreeMap::from([("description".to_owned(), "From two".to_owned())]);
    let problems = pick::apply(
        &http,
        &paths(dir.path()),
        &picking,
        &rom,
        &system,
        &picked,
        &[1],
    )
    .unwrap();
    assert!(problems.is_empty(), "{problems:?}");
    let list = std::fs::read_to_string(system.rom_path.join("gamelist.xml")).unwrap();
    assert!(list.contains("<desc>From two</desc>"), "{list}");
    assert!(
        list.contains("<thumbnail>./images/Sonic-thumb.png</thumbnail>"),
        "{list}"
    );
    assert!(!list.contains("<developer>"), "{list}");
    assert!(system.rom_path.join("images/Sonic-thumb.png").exists());
}

#[test]
fn details_are_edited_by_hand_and_a_field_can_be_cleared() {
    let dir = tempfile::tempdir().unwrap();
    let system = system(dir.path());
    let rom = game("Sonic.md", "Sonic");
    let paths = paths(dir.path());
    let set = BTreeMap::from([
        ("description".to_owned(), "A <fast> game".to_owned()),
        ("releaseDate".to_owned(), "1991-06".to_owned()),
    ]);
    pick::edit(&paths, &rom, &system, &set).unwrap();
    let list = std::fs::read_to_string(system.rom_path.join("gamelist.xml")).unwrap();
    assert!(list.contains("<desc>A &lt;fast&gt; game</desc>"), "{list}");
    assert!(
        list.contains("<releasedate>19910601T000000</releasedate>"),
        "{list}"
    );

    let clear = BTreeMap::from([("description".to_owned(), String::new())]);
    pick::edit(&paths, &rom, &system, &clear).unwrap();
    let list = std::fs::read_to_string(system.rom_path.join("gamelist.xml")).unwrap();
    assert!(!list.contains("<desc>"), "{list}");

    let no_name = BTreeMap::from([("name".to_owned(), " ".to_owned())]);
    assert!(pick::edit(&paths, &rom, &system, &no_name).is_err());
    let bad_date = BTreeMap::from([("releaseDate".to_owned(), "soon".to_owned())]);
    assert!(pick::edit(&paths, &rom, &system, &bad_date).is_err());
}

#[test]
fn deleting_a_game_takes_its_parts_entry_and_media_but_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let system = system(dir.path());
    let paths = paths(dir.path());
    let folder = system.rom_path.clone();
    std::fs::create_dir_all(folder.join("images")).unwrap();
    for file in [
        "Sonic.cue",
        "Sonic (Track 1).bin",
        "Other.md",
        "images/Sonic-thumb.png",
        "images/Other-thumb.png",
    ] {
        std::fs::write(folder.join(file), "x").unwrap();
    }
    std::fs::write(
        folder.join("Sonic.cue"),
        "FILE \"Sonic (Track 1).bin\" BINARY\n",
    )
    .unwrap();
    std::fs::write(
        folder.join("gamelist.xml"),
        "<gameList>\n\t<game>\n\t\t<path>./Sonic.cue</path>\n\t\t<name>Sonic</name>\n\t</game>\n\t<game>\n\t\t<path>./Other.md</path>\n\t\t<name>Other</name>\n\t</game>\n</gameList>\n",
    )
    .unwrap();
    let mut sonic = game("Sonic.cue", "Sonic");
    sonic.media.insert(
        "thumbnail".into(),
        folder
            .join("images/Sonic-thumb.png")
            .to_string_lossy()
            .into_owned(),
    );
    // A picture that is another game's, by mistake: left alone.
    sonic.media.insert(
        "image".into(),
        folder
            .join("images/Other-thumb.png")
            .to_string_lossy()
            .into_owned(),
    );
    crate::delete::delete_game(&paths, &sonic, &system).unwrap();

    assert!(!folder.join("Sonic.cue").exists());
    assert!(!folder.join("Sonic (Track 1).bin").exists());
    assert!(!folder.join("images/Sonic-thumb.png").exists());
    assert!(folder.join("Other.md").exists());
    assert!(folder.join("images/Other-thumb.png").exists());
    let list = std::fs::read_to_string(folder.join("gamelist.xml")).unwrap();
    assert!(!list.contains("Sonic"), "{list}");
    assert!(list.contains("<name>Other</name>"), "{list}");

    // Never out of the system's folder, nor the folder itself.
    for path in ["../Other.md", "/etc/passwd", ""] {
        let bad = game(path, "x");
        assert!(
            crate::delete::delete_game(&paths, &bad, &system).is_err(),
            "{path}"
        );
    }
    assert!(folder.join("Other.md").exists());
}

#[test]
fn deleting_a_playlist_takes_its_discs_their_entries_and_all_their_media() {
    let dir = tempfile::tempdir().unwrap();
    let system = system(&dir.path().join("dreamcast"));
    let paths = paths(dir.path());
    let folder = system.rom_path.clone();
    let outside = dir.path().join("outside.chd");
    std::fs::write(&outside, "x").unwrap();
    for sub in ["images", "videos", ".discs"] {
        std::fs::create_dir_all(folder.join(sub)).unwrap();
    }
    for file in [
        ".discs/Shenmue (Disc 1).cue",
        ".discs/Shenmue (Disc 1) (Track 1).bin",
        "Shenmue (Disc 2).chd",
        "Shenmue (Disc 3).chd",
        "images/Shenmue-thumb.png",
        "images/Shenmue-boxside.jpg",
        "videos/Shenmue-video.mp4",
        "images/Shenmue (Disc 1)-image.png",
        "images/Shenmue (Disc 2)-thumb.png",
        "images/Shenmue II-thumb.png",
    ] {
        std::fs::write(folder.join(file), "x").unwrap();
    }
    std::fs::write(
        folder.join(".discs/Shenmue (Disc 1).cue"),
        "FILE \"Shenmue (Disc 1) (Track 1).bin\" BINARY\n",
    )
    .unwrap();
    // Relative, in a folder, a full path, and one out of the system's folder (left alone).
    std::fs::write(
        folder.join("Shenmue.m3u"),
        format!(
            ".discs/Shenmue (Disc 1).cue\r\nShenmue (Disc 2).chd\r\n{}\r\n{}\r\n",
            folder.join("Shenmue (Disc 3).chd").display(),
            outside.display()
        ),
    )
    .unwrap();
    std::fs::write(
        folder.join("gamelist.xml"),
        "<gameList>\n\t<game>\n\t\t<path>./Shenmue.m3u</path>\n\t\t<name>Shenmue</name>\n\t</game>\n\t<game>\n\t\t<path>./Shenmue (Disc 2).chd</path>\n\t\t<name>Shenmue disc 2</name>\n\t</game>\n\t<game>\n\t\t<path>./Shenmue II.m3u</path>\n\t\t<name>Shenmue II</name>\n\t</game>\n</gameList>\n",
    )
    .unwrap();
    crate::delete::delete_game(&paths, &game("Shenmue.m3u", "Shenmue"), &system).unwrap();

    for gone in [
        "Shenmue.m3u",
        ".discs/Shenmue (Disc 1).cue",
        ".discs/Shenmue (Disc 1) (Track 1).bin",
        "Shenmue (Disc 2).chd",
        "Shenmue (Disc 3).chd",
        "images/Shenmue-thumb.png",
        "images/Shenmue-boxside.jpg",
        "videos/Shenmue-video.mp4",
        "images/Shenmue (Disc 1)-image.png",
        "images/Shenmue (Disc 2)-thumb.png",
    ] {
        assert!(!folder.join(gone).exists(), "{gone}");
    }
    assert!(outside.exists());
    assert!(folder.join("images/Shenmue II-thumb.png").exists());
    let list = std::fs::read_to_string(folder.join("gamelist.xml")).unwrap();
    assert!(!list.contains("Shenmue disc 2"), "{list}");
    assert!(!list.contains("<name>Shenmue</name>"), "{list}");
    assert!(list.contains("Shenmue II"), "{list}");
}

#[test]
fn a_themes_backup_comes_only_when_what_it_stands_in_for_cannot_be_had() {
    let run = |media: Vec<(MediaKind, String)>| {
        let dir = tempfile::tempdir().unwrap();
        let http = Serve::new(&[("https://a/wheel.png", PNG), ("https://a/marquee.png", PNG)]);
        let found = Found {
            metadata: Metadata::default(),
            media,
            notes: Default::default(),
        };
        let sources: Vec<Box<dyn Source>> = vec![Fake::new(
            "a",
            &[MediaKind::Wheel, MediaKind::Marquee],
            Ok(Some(found)),
        )];
        let mut cfg = config(&["a"], &[MediaKind::Wheel, MediaKind::Marquee]);
        cfg.needs.backups = vec![(MediaKind::Marquee, MediaKind::Wheel)];
        let out = scrape_game(
            &http,
            &paths(dir.path()),
            &sources,
            &cfg,
            &BTreeMap::new(),
            &game("Game.sfc", "Game"),
            &system(dir.path()),
            &mut HashSet::new(),
        )
        .unwrap();
        out.media
    };
    let wheel = (MediaKind::Wheel, "https://a/wheel.png".to_owned());
    let marquee = (MediaKind::Marquee, "https://a/marquee.png".to_owned());
    assert_eq!(run(vec![wheel, marquee.clone()]), [MediaKind::Wheel]);
    assert_eq!(run(vec![marquee]), [MediaKind::Marquee]);
}

#[test]
fn a_date_with_its_month_and_day_wins_from_whichever_source_has_it() {
    use super::date_precision;
    assert_eq!(date_precision(None), 0);
    assert_eq!(date_precision(Some("1994")), 1);
    assert_eq!(date_precision(Some("1994-03")), 2);
    assert_eq!(date_precision(Some("1994-03-19")), 3);
    assert_eq!(date_precision(Some("19940319T000000")), 3);

    let mut found = Metadata {
        release_date: Some("1994".into()),
        ..Metadata::default()
    };
    // The year alone: not done yet, the next source is asked for the day.
    assert!(!found.is_complete(false));
    found.fill_from(Metadata {
        release_date: Some("1994-03-19".into()),
        ..Metadata::default()
    });
    assert_eq!(found.release_date.as_deref(), Some("1994-03-19"));
    // Never the other way.
    found.fill_from(Metadata {
        release_date: Some("1995".into()),
        ..Metadata::default()
    });
    assert_eq!(found.release_date.as_deref(), Some("1994-03-19"));
}
