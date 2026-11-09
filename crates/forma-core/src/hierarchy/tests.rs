use super::*;
use crate::library::{Collection, Library};
use crate::test_support::{fixture_copy, fixture_paths};
use catalog::collections_view;

fn library() -> Library {
    let mut lib = Library::open_in_memory().unwrap();
    lib.refresh(&fixture_paths()).unwrap();
    lib
}

fn bundled(id: &str) -> View {
    load_views(&fixture_paths()).get(id).unwrap().clone()
}

fn browse(lib: &Library, view: &View, path: &[&str]) -> Browse {
    let path: Vec<String> = path.iter().map(|s| (*s).to_owned()).collect();
    lib.browse(view, &path, Page::default()).unwrap()
}

/// (key, label, count) of a groups listing.
fn groups(b: &Browse) -> Vec<(String, String, i64)> {
    match &b.listing {
        Listing::Groups { items, .. } => items
            .iter()
            .map(|g| (g.key.clone(), g.label.clone(), g.game_count))
            .collect(),
        other => panic!("expected groups, got {other:?}"),
    }
}

fn games(b: &Browse) -> Vec<String> {
    match &b.listing {
        Listing::Games { items, .. } => items.iter().map(|g| g.name.clone()).collect(),
        other => panic!("expected games, got {other:?}"),
    }
}

fn g(key: &str, label: &str, n: i64) -> (String, String, i64) {
    (key.to_owned(), label.to_owned(), n)
}

#[test]
fn every_bundled_view_parses() {
    let catalog = load_views(&fixture_paths());
    assert!(catalog.warnings.is_empty(), "{:?}", catalog.warnings);
    let ids: Vec<&str> = catalog.views.iter().map(|e| e.view.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "systems",
            "manufacturer-system",
            "genre",
            "all",
            "favorites",
            "recent",
            "recommended",
            "home",
            "collections"
        ]
    );
}

#[test]
fn the_systems_view_matches_emulationstation() {
    let lib = library();
    let view = bundled("systems");
    let root = browse(&lib, &view, &[]);
    assert_eq!(
        groups(&root),
        [
            g("nes", "Nintendo Entertainment System", 3),
            g("snes", "Super Famicom", 2),
            g("megadrive", "Mega Drive", 2),
            g("psx", "PlayStation", 1),
            g("homebrew", "Homebrew", 1),
        ]
    );
    let Listing::Groups { items, .. } = &root.listing else {
        unreachable!()
    };
    let mario = lib.search("super mario", 1).unwrap()[0].id;
    assert_eq!(items[0].image_game, Some(mario));
    assert_eq!(items[1].image_game, None);

    let nes = browse(&lib, &view, &["nes"]);
    assert_eq!(
        games(&nes),
        ["Homebrew Demo", "Super Mario Bros.", "The Legend of Zelda"]
    );
    assert_eq!(nes.trail[0].label, "Nintendo Entertainment System");
}

#[test]
fn manufacturer_then_system_with_unknown_last() {
    let lib = library();
    let view = bundled("manufacturer-system");
    assert_eq!(
        groups(&browse(&lib, &view, &[])),
        [
            g("Nintendo", "Nintendo", 5),
            g("Sega", "Sega", 2),
            g("Sony", "Sony", 1),
            g("", "Unknown", 1),
        ]
    );
    let nintendo = groups(&browse(&lib, &view, &["Nintendo"]));
    assert_eq!(
        nintendo.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(),
        ["nes", "snes"]
    );
    assert_eq!(
        games(&browse(&lib, &view, &["", "homebrew"])),
        ["Alter Ego"]
    );
}

#[test]
fn groups_sorted_by_count_keep_unknown_last() {
    let lib = library();
    assert_eq!(
        groups(&browse(&lib, &bundled("genre"), &[])),
        [
            g("Platform", "Platform", 2),
            g("Action / Adventure", "Action / Adventure", 1),
            g("Role Playing Game", "Role Playing Game", 1),
            g("", "Unknown", 5),
        ]
    );
}

#[test]
fn every_game_in_one_list_a_to_z() {
    let lib = library();
    let all = games(&browse(&lib, &bundled("all"), &[]));
    let mut sorted = all.clone();
    sorted.sort_by_key(|n| n.to_lowercase());
    assert!(all.len() > 1, "{all:?}");
    assert_eq!(all, sorted);
}

#[test]
fn filtered_views() {
    let lib = library();
    assert_eq!(
        games(&browse(&lib, &bundled("favorites"), &[])),
        ["Super Mario Bros."]
    );
    assert_eq!(
        games(&browse(&lib, &bundled("recent"), &[])),
        ["Super Mario Bros."]
    );
}

#[test]
fn a_branches_menu_counts_each_branch_and_opens_into_it() {
    let lib = library();
    let home = bundled("home");
    let Listing::Branches { items } = browse(&lib, &home, &[]).listing else {
        panic!("expected branches")
    };
    let counts: Vec<(&str, i64)> = items
        .iter()
        .map(|b| (b.id.as_str(), b.game_count))
        .collect();
    assert_eq!(
        counts,
        [
            ("favorites", 1),
            ("recent", 1),
            ("systems", 9),
            ("manufacturers", 9),
            ("genres", 9)
        ]
    );

    let sega = browse(&lib, &home, &["manufacturers", "Sega", "megadrive"]);
    assert_eq!(games(&sega).len(), 2);
    let labels: Vec<&str> = sega.trail.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["Manufacturers", "Sega", "Mega Drive"]);
}

#[test]
fn numeric_groups_sort_numerically_and_have_readable_labels() {
    let lib = library();
    let view = parse_view(
        r#"{ "id": "t", "name": "t", "root": {
              "groupBy": "decade", "children": {
              "groupBy": "players", "children": { "games": { "sort": "releaseDate" } } } } }"#,
    )
    .unwrap();
    assert_eq!(
        groups(&browse(&lib, &view, &[])),
        [
            g("1980", "1980s", 2),
            g("1990", "1990s", 1),
            g("", "Unknown", 6)
        ]
    );
    assert_eq!(
        groups(&browse(&lib, &view, &["1980"])),
        [g("1", "1 player", 1), g("2", "2 players", 1)]
    );
    assert_eq!(
        games(&browse(&lib, &view, &["1980", "2"])),
        ["Super Mario Bros."]
    );
}

#[test]
fn filters_combine_text_ranges_and_flags() {
    let lib = library();
    let view = parse_view(
        r#"{ "id": "t", "name": "t", "root": {
              "filter": { "manufacturer": ["nintendo", "Sony"], "year": { "min": 1986 } },
              "games": { "sort": "releaseDate", "order": "desc" } } }"#,
    )
    .unwrap();
    // Case-insensitive manufacturer match; Mario (1985) is excluded by the year.
    assert_eq!(
        games(&browse(&lib, &view, &[])),
        ["Final Fantasy VII", "The Legend of Zelda"]
    );
}

#[test]
fn pages_through_games_with_a_total() {
    let lib = library();
    let view = bundled("systems");
    let page = lib
        .browse(
            &view,
            &["nes".to_owned()],
            Page {
                offset: 1,
                limit: 1,
            },
        )
        .unwrap();
    let Listing::Games { items, total } = page.listing else {
        panic!()
    };
    assert_eq!(total, 3);
    assert_eq!(items[0].name, "Super Mario Bros.");
}

#[test]
fn a_bad_path_is_an_error_not_an_empty_page() {
    let lib = library();
    let home = bundled("home");
    let err = lib
        .browse(&home, &["nope".to_owned()], Page::default())
        .unwrap_err();
    assert!(err.to_string().contains("no branch `nope`"));
    let below = ["favorites".to_owned(), "x".to_owned()];
    assert!(lib.browse(&home, &below, Page::default()).is_err());
}

#[test]
fn accepts_a_schema_reference_for_editors() {
    let view = parse_view(
        r#"{ "$schema": "../view.schema.json", "id": "v", "name": "v", "root": { "games": {} } }"#,
    );
    assert!(view.is_ok(), "{view:?}");
}

#[test]
fn validation_says_where_the_problem_is() {
    let err = |json: &str| parse_view(json).unwrap_err();
    let wrap = |root: &str| format!(r#"{{ "id": "v", "name": "v", "root": {root} }}"#);

    assert!(err(&wrap(r#"{ "groupBy": "system", "games": {} }"#)).contains("exactly one of"));
    assert!(err(&wrap(r#"{ "groupBy": "system" }"#)).contains("root: `groupBy` needs `children`"));
    assert!(
        err(&wrap(
            r#"{ "groupBy": "colour", "children": { "games": {} } }"#
        ))
        .contains("colour")
    );
    assert!(err(&wrap(r#"{ "games": {}, "colour": 1 }"#)).contains("colour"));
    assert!(err(&wrap(r#"{ "games": { "limit": -1 } }"#)).contains("expected u32 at line"));
    assert!(
        err(&wrap(
            r#"{ "filter": { "year": { "min": 1999, "max": 1990 } }, "games": {} }"#
        ))
        .contains("min 1999 above max 1990")
    );
    assert!(
        err(&wrap(
            r#"{ "branches": [ { "id": "a", "name": "A", "children": { "games": {} } },
                               { "id": "a", "name": "B", "children": { "games": {} } } ] }"#
        ))
        .contains("root.branches[1]: id `a` is used twice")
    );
    assert!(
        err(&wrap(
            r#"{ "groupBy": "system", "children": { "groupBy": "genre" } }"#
        ))
        .contains("root.children:")
    );
    assert!(err(r#"{ "id": "Bad Id", "name": "v", "root": { "games": {} } }"#).contains("Bad Id"));

    let mut deep = String::from(r#"{ "games": {} }"#);
    for _ in 0..=MAX_DEPTH {
        deep = format!(r#"{{ "groupBy": "genre", "children": {deep} }}"#);
    }
    assert!(err(&wrap(&deep)).contains("deeper than"));
}

#[test]
fn user_views_add_to_and_replace_bundled_ones() {
    let (_dir, paths) = fixture_copy();
    let dir = paths.views_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("a-mine.json"),
        r#"{ "id": "mine", "name": "Mine", "root": { "groupBy": "decade", "children": { "games": {} } } }"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("b-systems.json"),
        r#"{ "id": "systems", "name": "My systems", "root": { "groupBy": "system", "sort": "name", "children": { "games": {} } } }"#,
    )
    .unwrap();
    std::fs::write(dir.join("c-broken.json"), "{ not json").unwrap();
    std::fs::write(
        dir.join("d-dupe.json"),
        r#"{ "id": "mine", "name": "Again", "root": { "games": {} } }"#,
    )
    .unwrap();

    let catalog = load_views(&paths);
    assert_eq!(catalog.get("mine").unwrap().name, "Mine");
    let systems = catalog
        .views
        .iter()
        .find(|e| e.view.id == "systems")
        .unwrap();
    assert_eq!(systems.view.name, "My systems");
    assert!(matches!(systems.origin, Origin::User(_)));
    assert_eq!(
        catalog.views[0].view.id, "systems",
        "replacement keeps its position"
    );
    assert_eq!(catalog.warnings.len(), 2, "{:?}", catalog.warnings);
}

#[test]
fn the_boot_view_follows_setting_then_theme_then_default() {
    let catalog = load_views(&fixture_paths());
    let boot = |setting, theme| {
        let (view, notes) = catalog.boot_view(setting, theme);
        (view.unwrap().id.clone(), notes.len())
    };
    assert_eq!(boot(None, None), ("systems".to_owned(), 0));
    assert_eq!(boot(None, Some("genre")), ("genre".to_owned(), 0));
    assert_eq!(
        boot(Some("manufacturer-system"), Some("genre")),
        ("manufacturer-system".to_owned(), 0)
    );
    // A deleted user view falls through, and says so.
    assert_eq!(boot(Some("gone"), Some("genre")), ("genre".to_owned(), 1));
    assert_eq!(
        boot(Some("gone"), Some("also-gone")),
        ("systems".to_owned(), 2)
    );
}

#[test]
fn the_boot_view_setting_is_stored_with_the_library() {
    let mut lib = library();
    assert_eq!(lib.setting(DEFAULT_VIEW_SETTING).unwrap(), None);
    lib.set_setting(DEFAULT_VIEW_SETTING, Some("manufacturer-system"))
        .unwrap();
    assert_eq!(
        lib.setting(DEFAULT_VIEW_SETTING).unwrap().as_deref(),
        Some("manufacturer-system")
    );
    lib.set_setting(DEFAULT_VIEW_SETTING, None).unwrap();
    assert_eq!(lib.setting(DEFAULT_VIEW_SETTING).unwrap(), None);
}

#[test]
fn collections_are_emulationstations_files_and_a_view_of_their_own() {
    let (_dir, paths) = fixture_copy();
    let mut lib = Library::open_in_memory().unwrap();
    lib.refresh(&paths).unwrap();
    let id = |q: &str| lib.search(q, 1).unwrap()[0].id;
    let (mario, zelda) = (id("super mario"), id("zelda"));

    // One made in EmulationStation: a full path per line, one for a game no longer here.
    let dir = paths.collections_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let rom = |game: i64| {
        let g = lib.game(game).unwrap().unwrap();
        let system = lib.launch_target(game).unwrap().unwrap().1;
        format!("{}/{}", system.rom_path.display(), g.path)
    };
    let (mario_rom, zelda_rom) = (rom(mario), rom(zelda));
    let zelda_name = lib.game(zelda).unwrap().unwrap().name;
    std::fs::write(
        dir.join("custom-Couch Co-op.cfg"),
        format!("{mario_rom}\n/userdata/roms/nes/Gone.nes\n\n"),
    )
    .unwrap();
    lib.refresh(&paths).unwrap();
    assert_eq!(
        lib.collections().unwrap(),
        [Collection {
            name: "Couch Co-op".into(),
            games: 1
        }]
    );
    assert_eq!(lib.collections_of(mario).unwrap(), ["Couch Co-op"]);

    // Made, filled and emptied from Forma: the file says the same.
    lib.create_collection(&paths, " RPGs ").unwrap();
    assert!(
        lib.create_collection(&paths, "rpgs").is_err(),
        "names are free of case"
    );
    lib.set_in_collection(&paths, "rpgs", zelda, true).unwrap();
    lib.set_in_collection(&paths, "Couch Co-op", zelda, true)
        .unwrap();
    lib.set_in_collection(&paths, "Couch Co-op", zelda, true)
        .unwrap();
    let file = std::fs::read_to_string(dir.join("custom-Couch Co-op.cfg")).unwrap();
    assert_eq!(
        file,
        format!("{mario_rom}\n/userdata/roms/nes/Gone.nes\n{zelda_rom}\n")
    );
    assert_eq!(lib.collections_of(zelda).unwrap(), ["Couch Co-op", "RPGs"]);

    let view = collections_view(&["Couch Co-op".into(), "RPGs".into()]);
    let root = browse(&lib, &view, &[]);
    let Listing::Branches { items } = &root.listing else {
        panic!("{root:?}")
    };
    let ids: Vec<(&str, i64)> = items
        .iter()
        .map(|b| (b.id.as_str(), b.game_count))
        .collect();
    assert_eq!(ids, [("couch-co-op", 2), ("rpgs", 1)]);
    assert_eq!(games(&browse(&lib, &view, &["rpgs"])), [zelda_name]);
    assert_eq!(
        load_views(&paths).get("collections").unwrap().root,
        view.root,
        "from the files"
    );

    lib.set_in_collection(&paths, "Couch Co-op", mario, false)
        .unwrap();
    assert_eq!(lib.collections_of(mario).unwrap(), Vec::<String>::new());
    lib.delete_collection(&paths, "rpgs").unwrap();
    assert!(!dir.join("custom-RPGs.cfg").exists());
    assert!(lib.delete_collection(&paths, "rpgs").is_err());
    for bad in ["", "a/b", "..", ".hidden", &"x".repeat(65)] {
        assert!(lib.create_collection(&paths, bad).is_err(), "{bad:?}");
    }
    assert!(
        lib.set_in_collection(&paths, "Couch Co-op", 99_999, true)
            .is_err()
    );

    // None at all: the view is an empty list, not an error.
    let empty = collections_view(&[]);
    assert!(games(&browse(&lib, &empty, &[])).is_empty());
}

#[test]
fn recommended_is_unplayed_games_like_the_ones_played() {
    let lib = library();
    let view = bundled("recommended");
    let all = games(&browse(&lib, &view, &[]));
    assert!(!all.is_empty());
    // Every visible game not played yet is a candidate.
    let unplayed: i64 = lib
        .conn
        .query_row(
            "SELECT COUNT(*) FROM game WHERE present = 1 AND hidden = 0
             AND play_count = 0 AND last_played IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(i64::try_from(all.len()).unwrap(), unplayed.min(50));

    // Hours on two games of one system and genre: its other games come first; those two go.
    let (system, played): (String, Vec<i64>) = {
        let (system, genre): (String, String) = lib
            .conn
            .query_row(
                "SELECT system, 'Puzzle' FROM game WHERE present = 1 AND hidden = 0
                 GROUP BY system ORDER BY COUNT(*) DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        lib.conn
            .execute(
                "UPDATE game SET genre = ?1 WHERE system = ?2",
                [&genre, &system],
            )
            .unwrap();
        let ids: Vec<i64> = lib
            .conn
            .prepare("SELECT id FROM game WHERE system = ?1 AND present = 1 AND hidden = 0 ORDER BY id LIMIT 2")
            .unwrap()
            .query_map([&system], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        (system, ids)
    };
    for id in &played {
        lib.conn
            .execute(
                "UPDATE game SET play_count = 5, game_time = 36000, last_played = '2026-11-01T20:00:00'
                 WHERE id = ?1",
                [id],
            )
            .unwrap();
    }
    let page = browse(&lib, &view, &[]);
    let Listing::Games { items, .. } = &page.listing else {
        panic!()
    };
    assert!(
        items.iter().all(|g| !played.contains(&g.id)),
        "played games are not recommended"
    );
    // Asked again with nothing changed: the kept answer, the same page.
    assert_eq!(browse(&lib, &view, &[]), page);
    let left: i64 = lib
        .conn
        .query_row(
            "SELECT COUNT(*) FROM game WHERE system = ?1 AND present = 1 AND hidden = 0
             AND play_count = 0",
            [&system],
            |r| r.get(0),
        )
        .unwrap();
    if left > 0 {
        assert_eq!(
            items[0].system, system,
            "the most like what is played comes first"
        );
    }
}

fn filtered(lib: &Library, view: &View, path: &[String], filter: &ListFilter) -> Vec<String> {
    games(
        &lib.browse_filtered(view, path, Page::default(), filter)
            .unwrap(),
    )
}

#[test]
fn a_list_narrows_down_by_name_letter_and_what_the_game_is() {
    let lib = library();
    let view = bundled("systems");
    let path = vec!["nes".to_owned()];
    let every = filtered(&lib, &view, &path, &ListFilter::default());
    assert!(every.len() >= 2, "the fixture has games to tell apart");
    let first = every[0].clone();
    lib.conn
        .execute(
            "UPDATE game SET genre = 'Action, Unusual / Puzzle', players_max = 4 WHERE name = ?1",
            [&first],
        )
        .unwrap();

    let name = ListFilter {
        name: Some(first[..2].to_lowercase()),
        ..Default::default()
    };
    assert!(filtered(&lib, &view, &path, &name).contains(&first));

    let letter = first.chars().next().unwrap().to_ascii_uppercase();
    let by_letter = ListFilter {
        letter: Some(letter.to_string()),
        ..Default::default()
    };
    let found = filtered(&lib, &view, &path, &by_letter);
    assert!(found.contains(&first));
    assert!(
        found
            .iter()
            .all(|n| n.to_ascii_uppercase().starts_with(letter))
    );

    for genre in ["Unusual", "puzzle"] {
        let by_genre = ListFilter {
            genre: Some(genre.to_owned()),
            ..Default::default()
        };
        assert_eq!(filtered(&lib, &view, &path, &by_genre), vec![first.clone()]);
    }

    // Literal words: `%` is not a wildcard.
    let none = ListFilter {
        name: Some("%".to_owned()),
        ..Default::default()
    };
    let options = lib.filter_options(&view, &path, &none).unwrap();
    assert_eq!((options.total, options.all), (0, every.len() as i64));

    let open = lib
        .filter_options(&view, &path, &ListFilter::default())
        .unwrap();
    assert_eq!(open.total, every.len() as i64);
    let count = |choices: &[FilterChoice], value: &str| {
        choices.iter().find(|c| c.value == value).map(|c| c.count)
    };
    assert_eq!(count(&open.genres, "Puzzle"), Some(1));
    assert_eq!(count(&open.genres, "Unusual"), Some(1));
    assert!(count(&open.letters, &letter.to_string()).is_some());
    // A game for four counts as a game for two.
    assert!(count(&open.players, "2").is_some_and(|n| n >= 1));
    assert!(open.players.iter().all(|c| c.value != "1"));
}

#[test]
fn only_a_game_list_can_be_filtered() {
    let lib = library();
    assert!(
        lib.filter_options(&bundled("systems"), &[], &ListFilter::default())
            .is_err()
    );
}
