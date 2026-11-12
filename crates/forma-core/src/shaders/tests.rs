use std::sync::{Arc, Mutex};

use super::*;

/// Records `batocera-settings-set` calls, and applies them to the file as the real one does.
struct FakeShell {
    conf: PathBuf,
    calls: Arc<Mutex<Vec<Vec<String>>>>,
}

impl Shell for FakeShell {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        assert_eq!(program, "batocera-settings-set");
        self.calls
            .lock()
            .unwrap()
            .push(args.iter().map(|a| (*a).to_owned()).collect());
        let text = std::fs::read_to_string(&self.conf).unwrap_or_default();
        let mut lines: Vec<String> = text
            .lines()
            .filter(|l| !l.starts_with(&format!("{}=", args[0])))
            .map(str::to_owned)
            .collect();
        lines.push(format!("{}={}", args[0], args[1]));
        std::fs::write(&self.conf, lines.join("\n") + "\n").unwrap();
        Ok(String::new())
    }
}

const CONFIG_INC: &str = r#"
#pragma parameter LABEL0000 "Read the documentation" 0 0 0 1
#pragma parameter IN_GLOW_GAMMA  "★ Gamma In"			2.0 	0.5 	10.0 0.01
#pragma parameter GAMMA_OUT      "★ Gamma out"			0.5		0.1 	5.0  0.01
#pragma parameter DO_BLOOM           "★ Bloom/Halation enable? (bl) ==>" 	0.0  0.0  1.0  1.0
#pragma parameter BLOOM_MIX          "     Bloom Strength"					0.45 0.0 1.0  0.01
#pragma parameter BLOOM_SIZE         "       . Radius"							4.0  0.25 30.0 0.25
#pragma parameter LABEL0023 " " 0 0 0 1
#pragma parameter DO_PIXELGRID		 "★  Low level Phosphor grid enable? (ph) ==>"	0.0 0.0 1.0 1.0
#pragma parameter DO_PIXELGRID_H            "      ★ Scanlines (===) (0 to disable)"    	1.0 0.0 1.25 0.05
#pragma parameter DO_BEZEL "★ Bezel enable? (bz) ==>" 0.0 0.0 1.0 1.0
#pragma parameter TATE "★ Tate, use horizontal scanlines (0:disabled 1:auto 2:forced)" 1.0 0.0 2.0 1.0
"#;

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    conf: PathBuf,
    calls: Arc<Mutex<Vec<Vec<String>>>>,
    shaders: Shaders,
}

fn fixture(conf_text: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let koko = root.join("batocera").join(KOKO);
    std::fs::create_dir_all(koko.join("shaders-ng")).unwrap();
    std::fs::create_dir_all(koko.join("Presets-ng")).unwrap();
    std::fs::create_dir_all(koko.join("Presets_Handhelds-ng")).unwrap();
    std::fs::write(koko.join(CONFIG), CONFIG_INC).unwrap();
    std::fs::write(
        koko.join("koko-aio-ng.slangp"),
        "shaders = 3\nGAMMA_OUT = \"0.45\"\n",
    )
    .unwrap();
    std::fs::write(
        koko.join("Presets-ng/Base.slangp"),
        "#reference \"../koko-aio-ng.slangp\"\nDO_BLOOM = \"1.0\"\n\tBLOOM_MIX = \"0.3\"\n",
    )
    .unwrap();
    std::fs::write(
        koko.join("Presets-ng/Monitor-Balanced.slangp"),
        "#reference \"Base.slangp\"\nBLOOM_MIX = \"0.2\"\n",
    )
    .unwrap();
    std::fs::write(
        koko.join("Presets_Handhelds-ng/GameboyAdvance.slangp"),
        "DO_PIXELGRID = \"0.0\"\n",
    )
    .unwrap();
    let conf = root.join("batocera.conf");
    std::fs::write(&conf, conf_text).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let shaders = Shaders::with_dirs(
        Box::new(FakeShell {
            conf: conf.clone(),
            calls: calls.clone(),
        }),
        &conf,
        &root.join("batocera"),
        &root.join("user"),
    );
    Fixture {
        _dir: dir,
        root,
        conf,
        calls,
        shaders,
    }
}

fn param<'p>(page: &'p ShaderPage, key: &str) -> &'p ShaderParam {
    page.sections
        .iter()
        .flat_map(|s| &s.params)
        .find(|p| p.key == key)
        .unwrap()
}

fn game() -> Scope {
    Scope::Game {
        system: "snes".into(),
        file: "Mario = World #1.sfc".into(),
    }
}

#[test]
fn reads_koko_aio_settings_in_its_sections() {
    let f = fixture("");
    let page = f.shaders.page(&Scope::Global).unwrap();
    assert!(page.available);
    let titles: Vec<_> = page.sections.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "General",
            "Bloom/Halation",
            "Low level Phosphor grid",
            "Bezel"
        ]
    );
    let general: Vec<_> = page.sections[0]
        .params
        .iter()
        .map(|p| p.key.as_str())
        .collect();
    // A starred setting at the left with no arrow is general, even after a section.
    assert_eq!(general, ["IN_GLOW_GAMMA", "GAMMA_OUT", "TATE"]);
    assert_eq!(page.sections[1].switch.as_deref(), Some("DO_BLOOM"));
    let bloom: Vec<_> = page.sections[1]
        .params
        .iter()
        .map(|p| p.label.as_str())
        .collect();
    assert_eq!(bloom, ["Bloom/Halation", "Bloom Strength", "Radius"]);
    // An indented star stays in its section: the scanlines are the phosphor grid's.
    assert_eq!(
        param(&page, "DO_PIXELGRID_H").label,
        "Scanlines (===) (0 to disable)"
    );
    assert_eq!(page.sections[2].params.len(), 2);
    assert_eq!(
        page.essentials,
        [
            "GAMMA_OUT",
            "DO_PIXELGRID",
            "DO_PIXELGRID_H",
            "DO_BLOOM",
            "BLOOM_MIX",
            "DO_BEZEL"
        ]
    );
    let presets: Vec<_> = page
        .preset
        .choices
        .iter()
        .map(|c| c.label.as_str())
        .collect();
    assert_eq!(
        presets,
        ["Base", "Monitor Balanced", "Handheld · GameboyAdvance"]
    );
}

#[test]
fn values_come_from_the_preset_chain_then_each_level() {
    let f = fixture("");
    let page = f.shaders.page(&Scope::Global).unwrap();
    assert_eq!(page.preset.inherited.value, DEFAULT_PRESET);
    // The preset's own value over its reference's; the root's over koko-aio's default.
    assert_eq!(param(&page, "BLOOM_MIX").inherited.value, 0.2);
    assert_eq!(param(&page, "BLOOM_MIX").inherited.from, "preset");
    assert_eq!(param(&page, "GAMMA_OUT").inherited.value, 0.45);
    assert_eq!(param(&page, "BLOOM_SIZE").inherited.from, "shader");

    f.shaders
        .set(&Scope::Global, "BLOOM_MIX", Some(0.6))
        .unwrap();
    f.shaders
        .set(&Scope::System("snes".into()), "BLOOM_SIZE", Some(8.0))
        .unwrap();
    let page = f.shaders.page(&game()).unwrap();
    assert_eq!(param(&page, "BLOOM_MIX").inherited.value, 0.6);
    assert_eq!(param(&page, "BLOOM_MIX").inherited.from, "global");
    assert_eq!(param(&page, "BLOOM_SIZE").inherited.from, "system");
    assert_eq!(param(&page, "BLOOM_SIZE").value, None);
    let page = f.shaders.page(&Scope::Global).unwrap();
    assert_eq!(param(&page, "BLOOM_MIX").value, Some(0.6));
}

#[test]
fn checks_settings_and_presets() {
    let f = fixture("");
    assert!(f.shaders.set(&Scope::Global, "NOPE", Some(1.0)).is_err());
    assert!(
        f.shaders
            .set(&Scope::Global, "BLOOM_MIX", Some(2.0))
            .is_err()
    );
    assert!(
        f.shaders
            .set_preset(&Scope::Global, Some("Presets-ng/Nope"))
            .is_err()
    );
    f.shaders
        .set_preset(&Scope::Global, Some("Presets-ng/Base"))
        .unwrap();
}

#[test]
fn switched_on_it_sets_its_shader_set_and_vulkan_and_puts_them_back() {
    let f = fixture("global.shaderset=scanlines\n");
    f.shaders.set_enabled(true).unwrap();
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.shaderset"], "forma");
    assert_eq!(conf["global.gfxbackend"], "vulkan");
    let user = f.root.join("user");
    let yml = std::fs::read_to_string(user.join("configs/forma/rendering-defaults.yml")).unwrap();
    assert!(yml.contains("default:\n  shader: forma/global\n"));
    // Handhelds have their own, from their screen's preset.
    assert!(yml.contains("gba:\n  shader: forma/systems/gba\n"));
    let gba = std::fs::read_to_string(user.join("forma/systems/gba.slangp")).unwrap();
    assert!(gba.contains("Presets_Handhelds-ng/GameboyAdvance.slangp\""));
    assert!(f.shaders.page(&Scope::Global).unwrap().enabled);

    f.shaders.set_enabled(false).unwrap();
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.shaderset"], "scanlines");
    assert!(!conf.contains_key("global.gfxbackend"));
}

#[test]
fn a_backend_the_user_chose_for_slang_is_kept() {
    let f = fixture("global.gfxbackend=glcore\n");
    f.shaders.set_enabled(true).unwrap();
    f.shaders.set_enabled(false).unwrap();
    assert_eq!(conf::read(&f.conf)["global.gfxbackend"], "glcore");
    let backend_sets = f
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c[0] == "global.gfxbackend")
        .count();
    assert_eq!(backend_sets, 0);
}

#[test]
fn writes_flattened_presets_and_a_games_key() {
    let f = fixture("");
    f.shaders.set_enabled(true).unwrap();
    f.shaders
        .set(&Scope::Global, "BLOOM_MIX", Some(0.6))
        .unwrap();
    f.shaders
        .set(&Scope::System("snes".into()), "BLOOM_SIZE", Some(8.0))
        .unwrap();
    f.shaders.set(&game(), "GAMMA_OUT", Some(1.0)).unwrap();
    let user = f.root.join("user/forma");
    let global = std::fs::read_to_string(user.join("global.slangp")).unwrap();
    assert!(global.contains("Presets-ng/Monitor-Balanced.slangp\""));
    assert!(global.contains("BLOOM_MIX = \"0.600000\""));
    let key = r#"snes["Mario  World 1.sfc"]-renderer.shader"#;
    let value = conf::read(&f.conf)[key].clone();
    let text =
        std::fs::read_to_string(user.parent().unwrap().join(format!("{value}.slangp"))).unwrap();
    // Every level's values, flattened over the preset.
    for line in [
        "BLOOM_MIX = \"0.600000\"",
        "BLOOM_SIZE = \"8.000000\"",
        "GAMMA_OUT = \"1.000000\"",
    ] {
        assert!(text.contains(line), "{line} in {text}");
    }
    // Unset, the game's key goes and its preset with it.
    f.shaders.set(&game(), "GAMMA_OUT", None).unwrap();
    assert!(!conf::read(&f.conf).contains_key(key));
    assert!(
        !user
            .parent()
            .unwrap()
            .join(format!("{value}.slangp"))
            .exists()
    );
}

#[test]
fn a_level_with_its_own_preset_starts_over_from_it() {
    let f = fixture("");
    f.shaders
        .set(&Scope::Global, "BLOOM_MIX", Some(0.6))
        .unwrap();
    let snes = Scope::System("snes".into());
    f.shaders
        .set_preset(&snes, Some("Presets-ng/Base"))
        .unwrap();
    let page = f.shaders.page(&snes).unwrap();
    assert_eq!(page.preset.value.as_deref(), Some("Presets-ng/Base"));
    assert_eq!(page.preset.inherited.value, DEFAULT_PRESET);
    // Global's change is not carried over to it: Base's own value.
    assert_eq!(param(&page, "BLOOM_MIX").inherited.value, 0.3);
    assert_eq!(param(&page, "BLOOM_MIX").inherited.from, "preset");
    // A game of it starts from it.
    let page = f.shaders.page(&game()).unwrap();
    assert_eq!(page.preset.inherited.value, "Presets-ng/Base");
    assert_eq!(page.preset.inherited.from, "system");
}

#[test]
fn without_koko_aio_it_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let conf = dir.path().join("batocera.conf");
    let shaders = Shaders::with_dirs(
        Box::new(FakeShell {
            conf: conf.clone(),
            calls: Arc::default(),
        }),
        &conf,
        &dir.path().join("none"),
        &dir.path().join("user"),
    );
    let page = shaders.page(&Scope::Global).unwrap();
    assert!(!page.available);
    assert!(shaders.set_enabled(true).is_err());
}

#[test]
fn koko_aio_s_bezel_turns_batocera_s_decoration_off_and_back() {
    let f = fixture("global.bezel=thebezelproject\nmegadrive.bezel=default\n");
    f.shaders.set_enabled(true).unwrap();
    // The shader on, but no bezel of its own: decorations as they were.
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.bezel"], "thebezelproject");
    assert_eq!(conf["megadrive.bezel"], "default");

    f.shaders
        .set(&Scope::Global, "DO_BEZEL", Some(1.0))
        .unwrap();
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.bezel"], "none");
    // A system's own decoration would still frame it: off too.
    assert_eq!(conf["megadrive.bezel"], "none");
    assert!(
        f.shaders
            .page(&Scope::System("megadrive".into()))
            .unwrap()
            .draws_bezel
    );

    // A system that turns koko-aio's bezel off gets the decoration it had from global.
    let snes = Scope::System("snes".into());
    f.shaders.set(&snes, "DO_BEZEL", Some(0.0)).unwrap();
    let conf = conf::read(&f.conf);
    assert_eq!(conf["snes.bezel"], "thebezelproject");
    assert!(!f.shaders.page(&snes).unwrap().draws_bezel);

    // Off: every key as the user had it.
    f.shaders.set_enabled(false).unwrap();
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.bezel"], "thebezelproject");
    assert_eq!(conf["megadrive.bezel"], "default");
    assert!(!conf.contains_key("snes.bezel"));
}

#[test]
fn a_decoration_the_user_picks_meanwhile_is_theirs() {
    let f = fixture("global.bezel=thebezelproject\n");
    f.shaders.set_enabled(true).unwrap();
    f.shaders
        .set(&Scope::Global, "DO_BEZEL", Some(1.0))
        .unwrap();
    // Changed by hand (or from the emulator options) while Forma had it off…
    FakeShell {
        conf: f.conf.clone(),
        calls: Arc::default(),
    }
    .run("batocera-settings-set", &["global.bezel", "arcade"])
    .unwrap();
    // …it is what comes back, not what was there before.
    f.shaders.set(&Scope::Global, "DO_BEZEL", None).unwrap();
    assert_eq!(conf::read(&f.conf)["global.bezel"], "arcade");
}

/// The fixture's shaders with the sample preset active (its shader on, global and NES levels).
fn with_preset(mut f: Fixture) -> Fixture {
    let presets = Presets::with(
        &f.root.join("presets.json"),
        vec![crate::presets::tests::sample()],
    );
    presets.set_active(Some("sample")).unwrap();
    f.shaders = f.shaders.with_presets(presets);
    f
}

#[test]
fn an_active_preset_switches_it_on_and_its_levels_sit_under_the_users() {
    let f = with_preset(fixture("global.shaderset=scanlines\n"));
    // On as the preset comes in, without the user switching it on.
    f.shaders.refresh().unwrap();
    assert!(f.shaders.page(&Scope::Global).unwrap().enabled);
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.shaderset"], "forma");
    assert_eq!(conf["global.gfxbackend"], "vulkan");

    // Its global value, then the user's over it.
    let page = f.shaders.page(&Scope::Global).unwrap();
    let gamma = param(&page, "GAMMA_OUT");
    assert_eq!((gamma.value, gamma.inherited.value), (None, 0.8));
    assert_eq!(gamma.inherited.from, "formaPreset");
    // Its NES value is nearer to NES than the user's global one; another system gets the
    // user's.
    f.shaders
        .set(&Scope::Global, "GAMMA_OUT", Some(0.5))
        .unwrap();
    let nes = f.shaders.page(&Scope::System("nes".into())).unwrap();
    assert_eq!(param(&nes, "GAMMA_OUT").inherited.value, 1.2);
    assert_eq!(param(&nes, "GAMMA_OUT").inherited.from, "formaPreset");
    let snes = f.shaders.page(&Scope::System("snes".into())).unwrap();
    assert_eq!(param(&snes, "GAMMA_OUT").inherited.value, 0.5);
    assert_eq!(param(&snes, "GAMMA_OUT").inherited.from, "global");
    // And a system of the preset's has its own file.
    let user = f.root.join("user");
    let nes = std::fs::read_to_string(user.join("forma/systems/nes.slangp")).unwrap();
    assert!(nes.contains("GAMMA_OUT = \"1.200000\""), "{nes}");
    let yml = std::fs::read_to_string(user.join("configs/forma/rendering-defaults.yml")).unwrap();
    assert!(yml.contains("nes:\n  shader: forma/systems/nes\n"));
    // Its bezel on: Batocera's decoration off (none set, which is none).
    assert!(f.shaders.page(&Scope::Global).unwrap().draws_bezel);
    assert!(!conf::read(&f.conf).contains_key("global.bezel"));
}

#[test]
fn switched_off_by_the_user_it_stays_off_whatever_the_preset_says() {
    let f = with_preset(fixture("global.shaderset=scanlines\n"));
    f.shaders.refresh().unwrap();
    f.shaders.set_enabled(false).unwrap();
    assert!(!f.shaders.page(&Scope::Global).unwrap().enabled);
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.shaderset"], "scanlines");
    assert!(!conf.contains_key("global.gfxbackend"));
    f.shaders.refresh().unwrap();
    assert!(!f.shaders.page(&Scope::Global).unwrap().enabled);

    // The preset gone, off as before; switched on, on whatever the preset.
    f.shaders.set_enabled(true).unwrap();
    assert!(f.shaders.page(&Scope::Global).unwrap().enabled);
}

#[test]
fn the_preset_gone_its_shader_goes_as_the_user_had_it() {
    let f = with_preset(fixture("global.shaderset=scanlines\nglobal.bezel=arcade\n"));
    f.shaders.refresh().unwrap();
    assert_eq!(conf::read(&f.conf)["global.bezel"], "none");
    let presets = f.shaders.presets.clone().unwrap();
    presets.set_active(None).unwrap();
    f.shaders.refresh().unwrap();
    assert!(!f.shaders.page(&Scope::Global).unwrap().enabled);
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.shaderset"], "scanlines");
    assert_eq!(conf["global.bezel"], "arcade");
}

#[test]
fn a_file_from_before_presets_keeps_its_shader_set() {
    let f = fixture("global.shaderset=forma\n");
    let dir = f.root.join("user/forma");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("forma.json"),
        r#"{ "enabled": true, "before_shaderset": "koko-aio", "set_backend": false }"#,
    )
    .unwrap();
    f.shaders
        .set(&Scope::Global, "GAMMA_OUT", Some(0.5))
        .unwrap();
    // Still on: nothing switched again, so what was there before is still koko-aio.
    assert!(
        !f.calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c[0] == "global.shaderset")
    );
    f.shaders.set_enabled(false).unwrap();
    assert_eq!(conf::read(&f.conf)["global.shaderset"], "koko-aio");
}
