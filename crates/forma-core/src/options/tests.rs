use std::path::Path;
use std::sync::Mutex;

use super::*;
use crate::systems::load_systems;
use crate::test_support::fixture_paths;

/// Records what would run, and answers `batocera-resolution listModes`; `batocera-settings-set`
/// is applied to the file, as the real one would.
struct FakeShell {
    conf: PathBuf,
    ran: Mutex<Vec<Vec<String>>>,
}

impl Shell for FakeShell {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        self.ran.lock().unwrap().push(
            std::iter::once(program)
                .chain(args.iter().copied())
                .map(str::to_owned)
                .collect(),
        );
        match (program, args) {
            ("batocera-resolution", ["listModes"]) => {
                Ok("max-1920x1080:maximum 1920x1080\n1920x1080.60000:1920x1080 60 Hz\n".into())
            }
            ("batocera-settings-set", [key, value]) => {
                let mut text = std::fs::read_to_string(&self.conf).unwrap_or_default();
                text.push_str(&format!("{key}={value}\n"));
                std::fs::write(&self.conf, text).unwrap();
                Ok(String::new())
            }
            _ => panic!("unexpected {program} {args:?}"),
        }
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    conf: PathBuf,
    options: Options,
    systems: Vec<System>,
}

fn fixture(conf_text: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let conf = dir.path().join("batocera.conf");
    std::fs::write(&conf, conf_text).unwrap();
    let shaders = dir.path().join("shaders");
    std::fs::create_dir_all(shaders.join("retro")).unwrap();
    std::fs::create_dir_all(shaders.join("scanlines")).unwrap();
    let decorations = dir.path().join("decorations");
    std::fs::create_dir_all(decorations.join("arcade_1980s")).unwrap();
    std::fs::write(decorations.join("readme.txt"), "").unwrap();

    let paths = fixture_paths();
    let features = paths.systems_cfg.with_file_name("es_features.cfg");
    let shell = FakeShell {
        conf: conf.clone(),
        ran: Mutex::new(Vec::new()),
    };
    let options = Options::new(Box::new(shell), &conf, &features, Path::new("/nonexistent"))
        .with_preset_dirs(vec![shaders], vec![decorations]);
    Fixture {
        systems: load_systems(&paths).unwrap().systems,
        _dir: dir,
        conf,
        options,
    }
}

impl Fixture {
    fn system(&self, name: &str) -> &System {
        self.systems.iter().find(|s| s.name == name).unwrap()
    }

    fn page(&self, scope: &Scope) -> OptionsPage {
        let system = scope.system().map(|s| self.system(s));
        self.options.page(scope, system).unwrap()
    }

    fn set(&self, scope: &Scope, key: &str, value: Option<&str>) -> Result<()> {
        let system = scope.system().map(|s| self.system(s));
        self.options.set(scope, system, key, value)
    }
}

fn keys(page: &OptionsPage) -> Vec<&str> {
    page.options.iter().map(|o| o.key.as_str()).collect()
}

fn item<'a>(page: &'a OptionsPage, key: &str) -> &'a OptionItem {
    page.options.iter().find(|o| o.key == key).unwrap()
}

fn nes_game() -> Scope {
    Scope::Game {
        system: "nes".into(),
        file: "Super Mario Bros. (World).nes".into(),
    }
}

#[test]
fn global_options_are_es_global_features_with_presets_resolved() {
    let f = fixture("");
    let page = f.page(&Scope::Global);
    // `future` has a preset this reader does not know: left out.
    assert_eq!(
        keys(&page),
        [
            "videomode",
            "ratio",
            "shaderset",
            "integerscale",
            "bezel",
            "rewind",
            "incrementalsavestates",
            "rumble_gain",
            "ai_service_url"
        ]
    );
    let modes: Vec<&str> = item(&page, "videomode")
        .choices
        .iter()
        .map(|c| c.value.as_str())
        .collect();
    assert_eq!(modes, ["max-1920x1080", "1920x1080.60000"]);
    let shaders: Vec<&str> = item(&page, "shaderset")
        .choices
        .iter()
        .map(|c| c.value.as_str())
        .collect();
    assert_eq!(shaders, ["none", "retro", "scanlines"]);
    let bezels: Vec<&str> = item(&page, "bezel")
        .choices
        .iter()
        .map(|c| c.label.as_str())
        .collect();
    assert_eq!(bezels, ["None", "arcade 1980s"]);
    let rumble = item(&page, "rumble_gain");
    assert_eq!(
        (
            rumble.kind,
            rumble.min,
            rumble.max,
            rumble.default.as_deref()
        ),
        ("number", Some(0.0), Some(100.0), Some("100"))
    );
    assert_eq!(item(&page, "integerscale").choices.len(), 2);
    assert_eq!(item(&page, "ai_service_url").kind, "text");
}

#[test]
fn a_system_offers_its_emulators_cores_and_the_chosen_cores_options() {
    let f = fixture("");
    let page = f.page(&Scope::System("nes".into()));
    assert_eq!(
        keys(&page),
        [
            "emulator",
            "core",
            "videomode",
            "ratio",
            "shaderset",
            "rewind",
            "autosave",
            "incrementalsavestates",
            "fceumm_palette"
        ]
    );
    // Batocera's own, which ES adds beside auto save: off writes `nes.incrementalsavestates=0`.
    let incremental = item(&page, "incrementalsavestates");
    assert_eq!(incremental.choices.len(), 2);
    f.set(
        &Scope::System("nes".into()),
        "incrementalsavestates",
        Some("0"),
    )
    .unwrap();
    assert_eq!(item(&page, "core").default.as_deref(), Some("fceumm"));
    assert_eq!(page.flags, ["netplay", "cheevos"]);

    // Another core, other options; a core's system-specific ones too.
    let psx = f.page(&Scope::System("psx".into()));
    assert!(keys(&psx).contains(&"swanstation_pgxp"));
    f.set(
        &Scope::System("psx".into()),
        "emulator",
        Some("duckstation"),
    )
    .unwrap();
    let psx = f.page(&Scope::System("psx".into()));
    assert_eq!(
        keys(&psx),
        ["emulator", "core", "videomode", "duckstation_gfxbackend"]
    );
}

#[test]
fn a_game_overrides_its_system_which_overrides_global() {
    let f = fixture("global.ratio=4/3\nnes.ratio=16/9\nglobal.rewind=1\n");
    let system = f.page(&Scope::System("nes".into()));
    let ratio = item(&system, "ratio");
    assert_eq!(ratio.value.as_deref(), Some("16/9"));
    assert_eq!(
        ratio.inherited,
        Some(Inherited {
            value: "4/3".into(),
            from: "global"
        })
    );

    let game = f.page(&nes_game());
    let ratio = item(&game, "ratio");
    assert_eq!(ratio.value, None);
    assert_eq!(ratio.inherited.as_ref().unwrap().from, "system");
    assert_eq!(
        item(&game, "rewind").inherited.as_ref().unwrap().from,
        "global"
    );

    f.set(&nes_game(), "ratio", Some("core")).unwrap();
    let text = std::fs::read_to_string(&f.conf).unwrap();
    assert!(
        text.contains("nes[\"Super Mario Bros. (World).nes\"].ratio=core\n"),
        "{text}"
    );
    assert_eq!(
        item(&f.page(&nes_game()), "ratio").value.as_deref(),
        Some("core")
    );

    // Unset: the game inherits again.
    f.set(&nes_game(), "ratio", None).unwrap();
    let ratio = item(&f.page(&nes_game()), "ratio").clone();
    assert_eq!(
        (ratio.value, ratio.inherited.unwrap().value),
        (None, "16/9".into())
    );
}

#[test]
fn values_are_checked_against_the_option() {
    let f = fixture("");
    let nes = Scope::System("nes".into());
    assert!(f.set(&nes, "ratio", Some("5/4")).is_err());
    assert!(
        f.set(&nes, "overclock_superfx", Some("200%")).is_err(),
        "another core's option"
    );
    assert!(
        f.set(&nes, "core", Some("snes9x")).is_err(),
        "another system's core"
    );
    assert!(f.set(&Scope::Global, "rumble_gain", Some("101")).is_err());
    assert!(f.set(&Scope::Global, "rumble_gain", Some("loud")).is_err());
    assert!(
        f.set(&Scope::Global, "ai_service_url", Some("a\nb"))
            .is_err()
    );
    assert!(f.set(&Scope::Global, "emulator", Some("libretro")).is_err());
    assert!(
        f.options
            .set(
                &Scope::System("dreamcast".into()),
                None,
                "ratio",
                Some("4/3")
            )
            .is_err()
    );
    f.set(&Scope::Global, "rumble_gain", Some("55")).unwrap();
    f.set(&nes, "core", Some("nestopia")).unwrap();
    assert_eq!(
        item(&f.page(&nes), "core").value.as_deref(),
        Some("nestopia")
    );
}

#[test]
fn changing_the_emulator_drops_a_core_set_at_the_same_level() {
    let f = fixture("psx.core=pcsx_rearmed\n");
    let psx = Scope::System("psx".into());
    f.set(&psx, "emulator", Some("duckstation")).unwrap();
    let page = f.page(&psx);
    assert_eq!(item(&page, "core").value, None);
    assert_eq!(item(&page, "core").default.as_deref(), Some("duckstation"));
}

#[test]
fn video_mode_labels_are_tidied() {
    assert_eq!(
        video_mode_label("1920x1080 59.939999 Hz"),
        "1920x1080 · 59.94 Hz"
    );
    assert_eq!(
        video_mode_label("1920x1080 60.000000 Hz"),
        "1920x1080 · 60 Hz"
    );
    assert_eq!(video_mode_label("maximum 1920x1080"), "Maximum 1920x1080");
}

#[test]
fn a_system_lists_every_emulator_and_core_as_one_choice() {
    let f = fixture("");
    let psx = f.page(&Scope::System("psx".into()));
    let labels: Vec<_> = psx.cores.iter().map(|c| c.label.as_str()).collect();
    assert!(labels.contains(&"swanstation"), "{labels:?}");
    assert!(labels.contains(&"duckstation (standalone)"), "{labels:?}");
    // One default: what the system runs when nothing is set.
    assert_eq!(psx.cores.iter().filter(|c| c.default).count(), 1);
    // Globally, nothing to choose.
    assert!(f.page(&Scope::Global).cores.is_empty());
}
