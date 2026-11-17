use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::test_support::fixture_copy;

fn pads() -> Vec<Controller> {
    vec![
        Controller {
            index: 0,
            guid: "030000005e0400008e02000014010000".into(),
            name: "Xbox 360 Controller".into(),
            device: Some(crate::input::Device {
                path: "/dev/input/event10".into(),
                buttons: 11,
                hats: 1,
                axes: 6,
            }),
        },
        Controller {
            index: 1,
            guid: "05000000c82d00000161000000010000".into(),
            name: "8BitDo SN30 Pro".into(),
            device: None,
        },
    ]
}

fn setup() -> (tempfile::TempDir, Paths, Library) {
    let (dir, paths) = fixture_copy();
    let mut lib = Library::open_in_memory().unwrap();
    lib.refresh(&paths).unwrap();
    (dir, paths, lib)
}

fn id_of(lib: &Library, name: &str) -> i64 {
    lib.search(name, 1).unwrap()[0].id
}

#[test]
fn builds_the_batocera_command_from_the_system_template() {
    let (_dir, paths, lib) = setup();
    let mario = id_of(&lib, "super mario");
    let prepared = prepare(&lib, &paths, mario, &LaunchRequest::default(), &pads()).unwrap();
    let cmd = &prepared.command;
    let rom = paths.roms().join("nes/Super Mario Bros. (World).nes");
    let game_xml = paths.forma_dir().join("run/game.xml");

    assert_eq!(cmd.program, "emulatorlauncher");
    let expected: Vec<String> = [
        "-p1index",
        "0",
        "-p1guid",
        "030000005e0400008e02000014010000",
        "-p1name",
        "Xbox 360 Controller",
        "-p1devicepath",
        "/dev/input/event10",
        "-p1nbbuttons",
        "11",
        "-p1nbhats",
        "1",
        "-p1nbaxes",
        "6",
        "-p2index",
        "1",
        "-p2guid",
        "05000000c82d00000161000000010000",
        "-p2name",
        "8BitDo SN30 Pro",
        "-system",
        "nes",
        "-rom",
        &rom.to_string_lossy(),
        "-gameinfoxml",
        &game_xml.to_string_lossy(),
        "-systemname",
        "Nintendo Entertainment System",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    assert_eq!(cmd.args, expected);
}

#[test]
fn writes_the_game_info_file_escaped() {
    let (_dir, paths, lib) = setup();
    // Hidden from views, but still launchable.
    let tom = lib.launch_target(
        lib.conn
            .query_row("SELECT id FROM game WHERE name = 'Tom & Jerry'", [], |r| {
                r.get(0)
            })
            .unwrap(),
    );
    let (game, _) = tom.unwrap().unwrap();
    prepare(&lib, &paths, game.id, &LaunchRequest::default(), &[]).unwrap();
    let xml = std::fs::read_to_string(paths.forma_dir().join("run/game.xml")).unwrap();
    assert!(xml.contains("<name>Tom &amp; Jerry</name>"), "{xml}");
    assert!(xml.contains("Tom &amp; Jerry (USA).nes</path>"), "{xml}");
}

#[test]
fn a_hostile_file_name_stays_one_argument() {
    let system = System {
        name: "nes".into(),
        fullname: "NES".into(),
        manufacturer: None,
        release_year: None,
        hardware: None,
        rom_path: "/r".into(),
        extensions: vec![],
        command: Some("emulatorlauncher -rom %ROM% -name \"%BASENAME% here\"".into()),
        platforms: vec![],
        theme: None,
        emulators: vec![],
    };
    let rom = Path::new("/r/$(rm -rf ~); echo pwned.nes");
    let p = Placeholders {
        game_info: Path::new("/g.xml"),
        home: Path::new("/h"),
        controllers: &[],
    };
    let cmd = build_command(&system, rom, &p, &LaunchRequest::default(), "/l".into()).unwrap();
    assert_eq!(
        cmd.args,
        [
            "-rom",
            "/r/$(rm -rf ~); echo pwned.nes",
            "-name",
            "$(rm -rf ~); echo pwned here"
        ]
    );
    assert_eq!(
        cmd.display(),
        "emulatorlauncher -rom '/r/$(rm -rf ~); echo pwned.nes' -name '$(rm -rf ~); echo pwned here'"
    );
}

#[test]
fn appends_overrides_and_netplay() {
    let (_dir, paths, lib) = setup();
    let zelda = id_of(&lib, "zelda");
    let host = LaunchRequest {
        emulator: Some("libretro".into()),
        core: Some("mesen".into()),
        netplay: Some(Netplay::Host {
            port: Some(55435),
            password: None,
        }),
        ignore_bios: false,
    };
    let args = prepare(&lib, &paths, zelda, &host, &[])
        .unwrap()
        .command
        .args;
    let tail: Vec<&str> = args
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(String::as_str)
        .collect();
    assert_eq!(
        tail,
        [
            "-emulator",
            "libretro",
            "-core",
            "mesen",
            "-netplaymode",
            "host",
            "-netplayport",
            "55435"
        ]
    );

    let client = LaunchRequest {
        netplay: Some(Netplay::Client {
            host: "192.168.1.20".into(),
            port: 55435,
            password: None,
            session: None,
            spectate: false,
        }),
        ..LaunchRequest::default()
    };
    let args = prepare(&lib, &paths, zelda, &client, &[])
        .unwrap()
        .command
        .args;
    let tail: Vec<&str> = args
        .iter()
        .rev()
        .take(6)
        .rev()
        .map(String::as_str)
        .collect();
    assert_eq!(
        tail,
        [
            "-netplaymode",
            "client",
            "-netplayip",
            "192.168.1.20",
            "-netplayport",
            "55435"
        ]
    );
}

#[test]
fn a_launch_request_reads_from_sdk_json() {
    let req: LaunchRequest =
        serde_json::from_str(r#"{ "netplay": { "mode": "client", "host": "h", "port": 1 } }"#)
            .unwrap();
    assert_eq!(
        req.netplay,
        Some(Netplay::Client {
            host: "h".into(),
            port: 1,
            password: None,
            session: None,
            spectate: false,
        })
    );
}

#[test]
fn joins_through_a_relay_with_a_password_and_refuses_odd_words() {
    let (_dir, paths, lib) = setup();
    let zelda = id_of(&lib, "zelda");
    let join = |host: &str, password: &str, session: &str, spectate| LaunchRequest {
        netplay: Some(Netplay::Client {
            host: host.into(),
            port: 55435,
            password: Some(password.into()),
            session: Some(session.into()),
            spectate,
        }),
        ..LaunchRequest::default()
    };
    let args = prepare(
        &lib,
        &paths,
        zelda,
        &join("relay.example", "s3cret", "abc123", true),
        &[],
    )
    .unwrap()
    .command
    .args;
    let tail: Vec<&str> = args
        .iter()
        .rev()
        .take(10)
        .rev()
        .map(String::as_str)
        .collect();
    assert_eq!(
        tail,
        [
            "-netplaymode",
            "spectator",
            "-netplayip",
            "relay.example",
            "-netplayport",
            "55435",
            "-netplaypass",
            "s3cret",
            "-netplaysession",
            "abc123"
        ]
    );
    for (host, password, session) in [
        ("-emulator", "p", "s"),
        ("h; reboot", "p", "s"),
        ("h", "two words", "s"),
        ("h", "-core", "s"),
        ("h", "p", "a\nb"),
    ] {
        assert!(
            prepare(
                &lib,
                &paths,
                zelda,
                &join(host, password, session, false),
                &[]
            )
            .is_err(),
            "{host} {password} {session}"
        );
    }
    let host = LaunchRequest {
        netplay: Some(Netplay::Host {
            port: None,
            password: Some("pw".into()),
        }),
        ..LaunchRequest::default()
    };
    let args = prepare(&lib, &paths, zelda, &host, &[])
        .unwrap()
        .command
        .args;
    assert!(args.ends_with(&[
        "-netplaymode".into(),
        "host".into(),
        "-netplaypass".into(),
        "pw".into()
    ]));
}

#[test]
fn an_unknown_game_or_a_system_without_command_is_refused_clearly() {
    let (_dir, paths, lib) = setup();
    let err = prepare(&lib, &paths, 9999, &LaunchRequest::default(), &[]).unwrap_err();
    assert!(err.to_string().contains("no game with id 9999"));

    let mut system = lib.launch_target(id_of(&lib, "zelda")).unwrap().unwrap().1;
    system.command = None;
    let p = Placeholders {
        game_info: Path::new("/g"),
        home: Path::new("/h"),
        controllers: &[],
    };
    let err = build_command(
        &system,
        Path::new("/x"),
        &p,
        &LaunchRequest::default(),
        "/l".into(),
    );
    assert!(err.unwrap_err().to_string().contains("has no <command>"));
}

#[test]
fn split_template_keeps_quoted_parts_together() {
    assert_eq!(
        split_template(r#"run  -a "b c"  -d"e f" """#),
        ["run", "-a", "b c", "-de f", ""]
    );
}

#[test]
fn a_finished_session_updates_play_stats_as_user_data() {
    let (_dir, paths, mut lib) = setup();
    let mario = id_of(&lib, "super mario");
    let ended = UNIX_EPOCH + Duration::from_secs(1_793_363_696);
    let exit = ExitInfo {
        code: Some(0),
        duration: Duration::from_secs(600),
    };
    finish(&mut lib, mario, &exit, ended).unwrap();

    let game = lib.game(mario).unwrap().unwrap();
    assert_eq!(game.play_count, 13, "12 from the gamelist, plus this one");
    assert_eq!(game.last_played.as_deref(), Some("2026-10-30T12:34:56"));
    assert_eq!(game.game_time, 600);

    // A scraper rewriting the gamelist does not undo it.
    let gamelist = paths.roms().join("nes/gamelist.xml");
    let xml = std::fs::read_to_string(&gamelist).unwrap();
    std::fs::write(&gamelist, xml.replace("<playcount>12", "<playcount>2")).unwrap();
    lib.refresh(&paths).unwrap();
    assert_eq!(lib.game(mario).unwrap().unwrap().play_count, 13);
}

#[test]
fn a_quick_failure_is_not_a_play_but_a_long_one_is() {
    let (_dir, _paths, mut lib) = setup();
    let zelda = id_of(&lib, "zelda");
    let crash = ExitInfo {
        code: Some(1),
        duration: Duration::from_secs(2),
    };
    finish(&mut lib, zelda, &crash, UNIX_EPOCH).unwrap();
    assert_eq!(lib.game(zelda).unwrap().unwrap().play_count, 0);

    // Killed after a real session (e.g. by the hotkey): still a play.
    let killed = ExitInfo {
        code: None,
        duration: Duration::from_secs(300),
    };
    finish(&mut lib, zelda, &killed, UNIX_EPOCH).unwrap();
    assert_eq!(lib.game(zelda).unwrap().unwrap().play_count, 1);
}

#[test]
fn the_mock_launcher_records_and_logs_what_it_would_run() {
    let (_dir, paths, lib) = setup();
    let prepared = prepare(
        &lib,
        &paths,
        id_of(&lib, "zelda"),
        &LaunchRequest::default(),
        &[],
    )
    .unwrap();
    let mock = MockLauncher::new(Duration::from_millis(10));
    let exit = mock.run(&prepared.command).unwrap();
    assert_eq!(exit.code, Some(0));
    assert_eq!(mock.ran(), std::slice::from_ref(&prepared.command));
    let log = std::fs::read_to_string(&prepared.command.log).unwrap();
    assert!(log.starts_with("[mock launcher] emulatorlauncher"), "{log}");
}

#[test]
fn a_missing_launcher_says_how_to_develop_without_batocera() {
    let (_dir, paths, lib) = setup();
    let mut cmd = prepare(
        &lib,
        &paths,
        id_of(&lib, "zelda"),
        &LaunchRequest::default(),
        &[],
    )
    .unwrap()
    .command;
    cmd.program = "forma-definitely-not-installed".into();
    let err = ProcessLauncher.run(&cmd).unwrap_err().to_string();
    assert!(err.contains("FORMA_LAUNCHER=mock"), "{err}");
}

#[test]
fn game_events_serialise_with_their_sdk_names() {
    let start = GameEvent::Start {
        game_id: 1,
        system: "nes".into(),
        name: "x".into(),
    };
    let json = serde_json::to_value(&start).unwrap();
    assert_eq!(json["type"], "game:start");
    assert_eq!(json["gameId"], 1);
}
