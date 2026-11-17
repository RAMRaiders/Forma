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

/// Forma's shader for every game, from the default preset.
fn on(f: &Fixture) {
    f.shaders
        .choose(&Scope::Global, Some("crt:Presets-ng/Monitor-Balanced"))
        .unwrap();
}

/// No shader for any game.
fn off(f: &Fixture) {
    f.shaders.choose(&Scope::Global, Some("none")).unwrap();
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
        .shader
        .choices
        .iter()
        .filter_map(|c| c.label.strip_prefix("CRT · "))
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
            .choose(&Scope::Global, Some("crt:Presets-ng/Nope"))
            .is_err()
    );
    f.shaders
        .choose(&Scope::Global, Some("crt:Presets-ng/Base"))
        .unwrap();
}

#[test]
fn switched_on_it_sets_its_shader_set_and_vulkan_and_none_puts_the_backend_back() {
    let f = fixture("global.shaderset=scanlines\n");
    on(&f);
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

    off(&f);
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.shaderset"], "none");
    assert!(!conf.contains_key("global.gfxbackend"));
}

#[test]
fn a_backend_the_user_chose_for_slang_is_kept() {
    let f = fixture("global.gfxbackend=glcore\n");
    on(&f);
    off(&f);
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
    on(&f);
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
        .choose(&snes, Some("crt:Presets-ng/Base"))
        .unwrap();
    let page = f.shaders.page(&snes).unwrap();
    assert_eq!(page.shader.value.as_deref(), Some("crt:Presets-ng/Base"));
    assert_eq!(page.shader.inherited.value, "default");
    // Global's change is not carried over to it: Base's own value.
    assert_eq!(param(&page, "BLOOM_MIX").inherited.value, 0.3);
    assert_eq!(param(&page, "BLOOM_MIX").inherited.from, "preset");
    // A game of it starts from it.
    let page = f.shaders.page(&game()).unwrap();
    assert_eq!(
        (
            page.shader.inherited.value.as_str(),
            page.shader.inherited.from
        ),
        ("crt:Presets-ng/Base", "system")
    );
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
    assert!(shaders.choose(&Scope::Global, Some("none")).is_err());
}

#[test]
fn koko_aio_s_bezel_turns_batocera_s_decoration_off_and_back() {
    let f = fixture("global.bezel=thebezelproject\nmegadrive.bezel=default\n");
    on(&f);
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
    off(&f);
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.bezel"], "thebezelproject");
    assert_eq!(conf["megadrive.bezel"], "default");
    assert!(!conf.contains_key("snes.bezel"));
}

#[test]
fn a_decoration_the_user_picks_meanwhile_is_theirs() {
    let f = fixture("global.bezel=thebezelproject\n");
    on(&f);
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

/// Batocera's sets in the fixture: one stock, one of the user's, and the stock koko-aio's.
fn with_sets(f: &Fixture) {
    for dir in [
        "batocera/configs/retro",
        "user/configs/koko-aio",
        "user/configs/mine",
    ] {
        std::fs::create_dir_all(f.root.join(dir)).unwrap();
    }
}

#[test]
fn one_choice_per_level_owns_batocera_s_shader_keys() {
    let f = fixture("");
    with_sets(&f);
    let snes = Scope::System("snes".into());
    let page = f.shaders.page(&Scope::Global).unwrap();
    let values: Vec<&str> = page
        .shader
        .choices
        .iter()
        .map(|c| c.value.as_str())
        .collect();
    // Batocera's default, none, Forma's from each preset, Batocera's own sets but its stock
    // koko-aio (Forma's is it); not the user's folder's.
    assert_eq!(
        values,
        [
            "default",
            "none",
            "crt:Presets-ng/Base",
            "crt:Presets-ng/Monitor-Balanced",
            "crt:Presets_Handhelds-ng/GameboyAdvance",
            "set:retro"
        ]
    );
    // Nothing chosen: Batocera's default, no key of Forma's.
    assert_eq!(page.shader.value.as_deref(), Some("default"));
    assert!(f.shaders.choose(&snes, Some("set:mine")).is_err());
    assert!(f.shaders.choose(&snes, Some("default")).is_err());
    assert!(f.shaders.choose(&Scope::Global, None).is_err());
    assert!(f.shaders.choose(&snes, Some("set:koko-aio")).is_err());

    // Forma's globally: its set, and Vulkan.
    f.shaders
        .choose(&Scope::Global, Some("crt:Presets-ng/Monitor-Balanced"))
        .unwrap();
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.shaderset"], "forma");
    assert_eq!(conf["global.gfxbackend"], "vulkan");
    let page = f.shaders.page(&snes).unwrap();
    assert!(page.enabled);
    assert_eq!(page.shader.value, None);
    assert_eq!(
        (
            page.shader.inherited.value.as_str(),
            page.shader.inherited.from
        ),
        ("crt:Presets-ng/Monitor-Balanced", "global")
    );

    // SNES with one of Batocera's, a game of it Forma's from another preset.
    f.shaders.choose(&snes, Some("set:retro")).unwrap();
    f.shaders
        .choose(&game(), Some("crt:Presets-ng/Base"))
        .unwrap();
    let conf = conf::read(&f.conf);
    assert_eq!(conf["snes.shaderset"], "retro");
    assert_eq!(conf[r#"snes["Mario  World 1.sfc"].shaderset"#], "forma");
    assert!(conf.contains_key(r#"snes["Mario  World 1.sfc"]-renderer.shader"#));
    let page = f.shaders.page(&game()).unwrap();
    assert_eq!(page.shader.value.as_deref(), Some("crt:Presets-ng/Base"));
    assert_eq!(
        (
            page.shader.inherited.value.as_str(),
            page.shader.inherited.from
        ),
        ("set:retro", "system")
    );

    // None for SNES; then back to what they inherit: no keys of their own.
    f.shaders.choose(&snes, Some("none")).unwrap();
    assert_eq!(conf::read(&f.conf)["snes.shaderset"], "none");
    f.shaders.choose(&game(), None).unwrap();
    f.shaders.choose(&snes, None).unwrap();
    let conf = conf::read(&f.conf);
    assert!(!conf.contains_key("snes.shaderset"));
    assert!(!conf.contains_key(r#"snes["Mario  World 1.sfc"].shaderset"#));
    assert!(!conf.contains_key(r#"snes["Mario  World 1.sfc"]-renderer.shader"#));

    // None globally: no shader, the backend as it was.
    f.shaders.choose(&Scope::Global, Some("none")).unwrap();
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.shaderset"], "none");
    assert!(!conf.contains_key("global.gfxbackend"));

    // Batocera's default globally: no key, Batocera's own applies.
    f.shaders.choose(&Scope::Global, Some("default")).unwrap();
    assert!(!conf::read(&f.conf).contains_key("global.shaderset"));
    let page = f.shaders.page(&Scope::Global).unwrap();
    assert_eq!(page.shader.value.as_deref(), Some("default"));
}

#[test]
fn picking_the_preset_it_would_start_from_keeps_the_settings_above() {
    let f = fixture("");
    let snes = Scope::System("snes".into());
    f.shaders
        .choose(&Scope::Global, Some("crt:Presets-ng/Base"))
        .unwrap();
    f.shaders
        .set(&Scope::Global, "GAMMA_OUT", Some(0.8))
        .unwrap();
    // Global's own preset, picked again for SNES: no preset of its own, global's settings stay.
    f.shaders
        .choose(&snes, Some("crt:Presets-ng/Base"))
        .unwrap();
    let page = f.shaders.page(&snes).unwrap();
    assert_eq!(page.shader.value.as_deref(), Some("crt:Presets-ng/Base"));
    assert_eq!(param(&page, "GAMMA_OUT").inherited.value, 0.8);
}

#[test]
fn batocera_s_keys_are_taken_in_as_choices() {
    let f = fixture(concat!(
        "global.shaderset=koko-aio\n",
        "snes.shaderset=retro\n",
        "nes.shaderset=none\n",
        "snes[\"Zelda.sfc\"].shaderset=koko-aio\n",
    ));
    with_sets(&f);
    let page = f.shaders.page(&Scope::Global).unwrap();
    assert!(page.enabled);
    assert_eq!(
        page.shader.value.as_deref(),
        Some("crt:Presets-ng/Monitor-Balanced")
    );
    let snes = f.shaders.page(&Scope::System("snes".into())).unwrap();
    assert_eq!(snes.shader.value.as_deref(), Some("set:retro"));
    let nes = f.shaders.page(&Scope::System("nes".into())).unwrap();
    assert_eq!(nes.shader.value.as_deref(), Some("none"));
    let zelda = Scope::Game {
        system: "snes".into(),
        file: "Zelda.sfc".into(),
    };
    let zelda = f.shaders.page(&zelda).unwrap();
    assert!(zelda.enabled);
    assert_eq!(zelda.shader.inherited.value, "set:retro");

    // Written as Forma's once, at start; not again after.
    f.shaders.adopt().unwrap();
    let conf = conf::read(&f.conf);
    assert_eq!(conf["global.shaderset"], "forma");
    assert_eq!(conf["snes.shaderset"], "retro");
    assert_eq!(conf["nes.shaderset"], "none");
    std::fs::write(&f.conf, "global.shaderset=retro\n").unwrap();
    f.shaders.adopt().unwrap();
    assert_eq!(conf::read(&f.conf)["global.shaderset"], "retro");
}
