use std::path::Path;

use super::*;
use crate::test_support::{fixture_copy, fixture_paths};

fn refreshed() -> (Library, RefreshReport) {
    let mut lib = Library::open_in_memory().unwrap();
    let report = lib.refresh(&fixture_paths()).unwrap();
    (lib, report)
}

fn by_name<'a>(games: &'a [Game], name: &str) -> &'a Game {
    games
        .iter()
        .find(|g| g.name == name)
        .unwrap_or_else(|| panic!("no game named {name}"))
}

#[test]
fn indexes_the_fixture_library() {
    let (lib, report) = refreshed();
    assert_eq!(report.systems, 5);
    assert_eq!(report.games, 10);
    assert_eq!(report.added, 10);
    assert_eq!(report.gamelists_imported, 3);

    let systems: Vec<String> = lib.systems().unwrap().into_iter().map(|s| s.name).collect();
    assert_eq!(systems, ["nes", "snes", "megadrive", "psx", "homebrew"]);
}

#[test]
fn reports_what_it_skipped() {
    let (_, report) = refreshed();
    let messages: Vec<&str> = report.warnings.iter().map(|w| w.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("no <path>")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("not on disk")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("entries before it were kept")),
        "{messages:?}"
    );
}

#[test]
fn applies_gamelist_metadata_and_media() {
    let (lib, _) = refreshed();
    let nes = lib.games("nes").unwrap();
    let mario = by_name(&nes, "Super Mario Bros.");
    assert_eq!(mario.release_date.as_deref(), Some("1985-09-13"));
    assert_eq!(mario.players.as_deref(), Some("1-2"));
    assert!(mario.favorite);
    assert_eq!(mario.play_count, 12);
    assert_eq!(mario.ra_game_id, Some(1446));
    assert!(mario.media["image"].ends_with("images/Super Mario Bros. (World)-image.png"));
    assert!(mario.media.contains_key("video"));
}

#[test]
fn a_game_without_metadata_is_named_after_its_file() {
    let (lib, _) = refreshed();
    let nes = lib.games("nes").unwrap();
    by_name(&nes, "Homebrew Demo");
    // Truncated snes gamelist: Chrono Trigger's entry was cut, so it falls back to the file name.
    by_name(&lib.games("snes").unwrap(), "Chrono Trigger (USA)");
    // No gamelist at all.
    assert_eq!(lib.games("megadrive").unwrap().len(), 2);
}

#[test]
fn hidden_games_are_not_listed() {
    let (lib, _) = refreshed();
    let nes = lib.games("nes").unwrap();
    assert!(nes.iter().all(|g| g.name != "Tom & Jerry"));
    assert_eq!(lib.systems().unwrap()[0].game_count, 3);
}

#[test]
fn search_matches_prefixes_and_ignores_accents() {
    let (lib, _) = refreshed();
    let names = |q: &str| -> Vec<String> {
        lib.search(q, 10)
            .unwrap()
            .into_iter()
            .map(|g| g.name)
            .collect()
    };
    assert_eq!(names("super mar"), ["Super Mario Bros."]);
    assert_eq!(names("cafe"), ["Super Mario Bros."]); // description says "Café"
    assert_eq!(names("square"), ["Final Fantasy VII"]); // developer
    assert!(names("  ").is_empty());
    assert!(names("\"unbalanced").is_empty());
}

#[test]
fn a_second_refresh_skips_unchanged_gamelists() {
    let mut lib = Library::open_in_memory().unwrap();
    lib.refresh(&fixture_paths()).unwrap();
    let report = lib.refresh(&fixture_paths()).unwrap();
    assert_eq!(report.added, 0);
    assert_eq!(report.gamelists_imported, 0);
    assert_eq!(report.gamelists_unchanged, 3);
}

#[test]
fn the_users_favourite_survives_a_gamelist_reimport() {
    let (_dir, paths) = fixture_copy();
    let mut lib = Library::open_in_memory().unwrap();
    lib.refresh(&paths).unwrap();
    let mario = by_name(&lib.games("nes").unwrap(), "Super Mario Bros.").id;

    lib.set_favorite(mario, false).unwrap();
    assert!(!lib.game(mario).unwrap().unwrap().favorite);

    // A scraper rewrites the gamelist, still saying favorite=true.
    let gamelist = paths.roms().join("nes/gamelist.xml");
    let xml = std::fs::read_to_string(&gamelist).unwrap();
    std::fs::write(
        &gamelist,
        xml.replace("Super Mario Bros.</name>", "SMB</name>"),
    )
    .unwrap();
    let report = lib.refresh(&paths).unwrap();
    assert!(report.gamelists_imported >= 1);

    let game = lib.game(mario).unwrap().unwrap();
    assert_eq!(game.name, "SMB", "the new gamelist was imported");
    assert!(!game.favorite, "but the user's choice still wins");
}

#[test]
fn a_missing_file_keeps_its_row_and_comes_back_with_its_data() {
    let (_dir, paths) = fixture_copy();
    let mut lib = Library::open_in_memory().unwrap();
    lib.refresh(&paths).unwrap();
    let zelda = by_name(&lib.games("nes").unwrap(), "The Legend of Zelda").id;
    lib.set_favorite(zelda, true).unwrap();

    let rom = paths.roms().join("nes/The Legend of Zelda (USA).nes");
    std::fs::rename(&rom, rom.with_extension("bak")).unwrap();
    let report = lib.refresh(&paths).unwrap();
    assert_eq!(report.missing, 1);
    assert!(lib.games("nes").unwrap().iter().all(|g| g.id != zelda));

    std::fs::rename(rom.with_extension("bak"), &rom).unwrap();
    lib.refresh(&paths).unwrap();
    let back = lib.game(zelda).unwrap().unwrap();
    assert!(back.favorite);
}

#[test]
fn a_new_rom_picks_up_an_existing_gamelist_entry() {
    let (_dir, paths) = fixture_copy();
    let mut lib = Library::open_in_memory().unwrap();
    lib.refresh(&paths).unwrap();
    // The gamelist already lists "Deleted Game (USA).nes"; now the file appears.
    std::fs::write(paths.roms().join("nes/Deleted Game (USA).nes"), b"").unwrap();
    lib.refresh(&paths).unwrap();
    by_name(&lib.games("nes").unwrap(), "Listed but not on disk");
}

#[test]
fn a_removed_system_takes_its_games_with_it() {
    let (_dir, paths) = fixture_copy();
    let mut lib = Library::open_in_memory().unwrap();
    lib.refresh(&paths).unwrap();
    std::fs::remove_file(paths.systems_overlay_dir().join("es_systems_custom.cfg")).unwrap();
    lib.refresh(&paths).unwrap();
    assert!(lib.systems().unwrap().iter().all(|s| s.name != "homebrew"));
    assert!(lib.games("homebrew").unwrap().is_empty());
}

#[test]
fn reopens_a_database_on_disk_and_refuses_a_newer_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/library.db");
    {
        let mut lib = Library::open(&path).unwrap();
        lib.refresh(&fixture_paths()).unwrap();
    }
    let lib = Library::open(&path).unwrap();
    assert_eq!(lib.games("nes").unwrap().len(), 3);
    drop(lib);

    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    assert!(matches!(Library::open(&path), Err(Error::Parse { .. })));
}

#[test]
fn helpers() {
    assert_eq!(players_max("1-2"), Some(2));
    assert_eq!(players_max("2+"), Some(2));
    assert_eq!(players_max("one"), None);
    assert_eq!(default_name("sub/Game (USA).nes"), "Game (USA)");
    assert_eq!(default_name(&Path::new("x").to_string_lossy()), "x");
}

#[test]
fn search_follows_renames_new_games_and_deletions() {
    let (_dir, paths) = fixture_copy();
    let mut lib = Library::open_in_memory().unwrap();
    lib.refresh(&paths).unwrap();
    let hits = |lib: &Library, q: &str| lib.search(q, 10).unwrap().len();

    // Renamed by a scraper: the old name stops matching, the new one starts.
    let gamelist = paths.roms().join("nes/gamelist.xml");
    let xml = std::fs::read_to_string(&gamelist).unwrap();
    std::fs::write(
        &gamelist,
        xml.replace("The Legend of Zelda</name>", "Hyrule Saga</name>"),
    )
    .unwrap();
    lib.refresh(&paths).unwrap();
    assert_eq!(hits(&lib, "zelda"), 0);
    assert_eq!(hits(&lib, "hyrule saga"), 1);

    // A new file is searchable straight away, under its file name.
    std::fs::write(paths.roms().join("megadrive/Ecco the Dolphin.md"), b"").unwrap();
    lib.refresh(&paths).unwrap();
    assert_eq!(hits(&lib, "ecco"), 1);

    // A removed system's games leave the index, so a reused rowid cannot match them.
    std::fs::remove_file(paths.systems_overlay_dir().join("es_systems_custom.cfg")).unwrap();
    lib.refresh(&paths).unwrap();
    assert_eq!(hits(&lib, "alter ego"), 0);
    let raw: i64 = lib
        .conn
        .query_row(
            "SELECT COUNT(*) FROM game_fts WHERE game_fts MATCH 'alter'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(raw, 0);
}

#[test]
fn a_favourite_change_does_not_touch_the_search_index() {
    let (mut lib, _) = refreshed();
    let mario = lib.search("mario", 1).unwrap()[0].id;
    lib.set_favorite(mario, false).unwrap();
    assert_eq!(lib.search("mario", 10).unwrap().len(), 1);
}

#[test]
fn reports_which_systems_changed() {
    let (_dir, paths) = fixture_copy();
    let mut lib = Library::open_in_memory().unwrap();
    let first = lib.refresh(&paths).unwrap();
    assert_eq!(first.changed_systems.len(), 5);

    assert!(lib.refresh(&paths).unwrap().changed_systems.is_empty());

    let rom = paths.roms().join("megadrive/Streets of Rage 2 (USA).md");
    std::fs::remove_file(&rom).unwrap();
    assert_eq!(lib.refresh(&paths).unwrap().changed_systems, ["megadrive"]);
    // Still missing: not a new change.
    assert!(lib.refresh(&paths).unwrap().changed_systems.is_empty());
    std::fs::write(&rom, b"").unwrap();
    assert_eq!(lib.refresh(&paths).unwrap().changed_systems, ["megadrive"]);
}

#[test]
fn refresh_systems_scans_only_the_named_ones() {
    let (_dir, paths) = fixture_copy();
    let mut lib = Library::open_in_memory().unwrap();
    lib.refresh(&paths).unwrap();
    std::fs::write(paths.roms().join("nes/New.nes"), b"").unwrap();
    std::fs::write(paths.roms().join("megadrive/New.md"), b"").unwrap();

    let report = lib
        .refresh_systems(&paths, &["megadrive".to_owned()])
        .unwrap();
    assert_eq!(report.systems, 1);
    assert_eq!(report.changed_systems, ["megadrive"]);
    assert_eq!(lib.games("nes").unwrap().len(), 3, "nes was not scanned");
    assert_eq!(lib.games("megadrive").unwrap().len(), 3);
}

fn drop_import(paths: &crate::paths::Paths, name: &str, body: &str) {
    std::fs::create_dir_all(paths.import_dir()).unwrap();
    std::fs::write(paths.import_dir().join(name), body).unwrap();
}

const HEADER: &str = r#"{"format":"romdex-import","formatVersion":1,"source":"test"}"#;

#[test]
fn imports_metadata_hashes_and_ra_ids_and_reports_what_it_skipped() {
    let (_dir, paths) = fixture_copy();
    let sample = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/import/retroworker.jsonl"),
    )
    .unwrap();
    drop_import(&paths, "retroworker.jsonl", &sample);
    let mut lib = Library::open_in_memory().unwrap();
    let report = lib.refresh(&paths).unwrap();
    assert_eq!((report.imports_applied, report.import_records), (1, 3));

    let sonic = by_name(&lib.games("megadrive").unwrap(), "Sonic the Hedgehog").clone();
    assert_eq!(
        sonic.genre.as_deref(),
        Some("Platform"),
        "a system with no gamelist"
    );
    assert_eq!(sonic.ra_game_id, Some(1));
    let zelda = by_name(&lib.games("nes").unwrap(), "The Legend of Zelda").clone();
    assert_eq!(zelda.ra_game_id, Some(1447));
    assert_eq!(
        zelda.rating,
        Some(0.85),
        "fields the import does not set keep the gamelist's"
    );
    assert!(zelda.media["marquee"].ends_with("nes/images/zelda-marquee.png"));

    let messages: Vec<&str> = report.warnings.iter().map(|w| w.message.as_str()).collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("not in the library yet")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.starts_with("line 4:")),
        "{messages:?}"
    );

    let again = lib.refresh(&paths).unwrap();
    assert_eq!(again.imports_applied, 0, "unchanged files are not re-read");
}

#[test]
fn an_import_outranks_the_gamelist_and_the_user_outranks_both() {
    let (_dir, paths) = fixture_copy();
    let mut lib = Library::open_in_memory().unwrap();
    lib.refresh(&paths).unwrap();
    let mario = by_name(&lib.games("nes").unwrap(), "Super Mario Bros.").id;
    lib.set_favorite(mario, false).unwrap();

    drop_import(
        &paths,
        "a.jsonl",
        &format!(
            "{HEADER}\n{}",
            r#"{"system":"nes","path":"./Super Mario Bros. (World).nes","metadata":{"name":"SMB (imported)","genre":"Action"}}"#
        ),
    );
    let report = lib.refresh(&paths).unwrap();
    assert_eq!(report.changed_systems, ["nes"]);
    let game = lib.game(mario).unwrap().unwrap();
    assert_eq!(game.name, "SMB (imported)");
    assert_eq!(game.genre.as_deref(), Some("Action"));
    assert_eq!(
        game.developer.as_deref(),
        Some("Nintendo"),
        "gamelist shows through"
    );
    assert!(
        !game.favorite,
        "the user's choice still wins over the gamelist's favorite"
    );

    // A later file wins where two set the same field.
    drop_import(
        &paths,
        "b.jsonl",
        &format!(
            "{HEADER}\n{}",
            r#"{"system":"nes","path":"Super Mario Bros. (World).nes","metadata":{"genre":"Platform"}}"#
        ),
    );
    lib.refresh(&paths).unwrap();
    let game = lib.game(mario).unwrap().unwrap();
    assert_eq!(
        (game.name.as_str(), game.genre.as_deref()),
        ("SMB (imported)", Some("Platform"))
    );

    // Remove the files: the gamelist's values are back.
    std::fs::remove_file(paths.import_dir().join("a.jsonl")).unwrap();
    std::fs::remove_file(paths.import_dir().join("b.jsonl")).unwrap();
    lib.refresh(&paths).unwrap();
    let game = lib.game(mario).unwrap().unwrap();
    assert_eq!(
        (game.name.as_str(), game.genre.as_deref()),
        ("Super Mario Bros.", Some("Platform"))
    );
    assert_eq!(
        lib.search("imported", 5).unwrap().len(),
        0,
        "the search index followed"
    );
}

#[test]
fn a_record_waits_for_its_file_to_appear() {
    let (_dir, paths) = fixture_copy();
    drop_import(
        &paths,
        "a.jsonl",
        &format!(
            "{HEADER}\n{}",
            r#"{"system":"nes","path":"Later (USA).nes","metadata":{"name":"Arrived Later"}}"#
        ),
    );
    let mut lib = Library::open_in_memory().unwrap();
    lib.refresh(&paths).unwrap();
    std::fs::write(paths.roms().join("nes/Later (USA).nes"), b"").unwrap();
    let report = lib.refresh(&paths).unwrap();
    assert_eq!(report.import_records, 1);
    by_name(&lib.games("nes").unwrap(), "Arrived Later");
}
