use super::*;

fn preset(json: &str) -> Preset {
    serde_json::from_str(json).unwrap()
}

/// A preset for every system, and for NES and SNES.
pub(crate) fn sample() -> Preset {
    preset(
        r#"{
          "id": "sample",
          "name": "Sample",
          "description": "For tests",
          "options": {
            "global": { "ratio": "4/3", "rewind": "0" },
            "systems": { "nes": { "ratio": "16/9" }, "snes": { "shaderset": "none" } }
          },
          "shader": {
            "enabled": true,
            "global": { "values": { "GAMMA_OUT": 0.8, "DO_BEZEL": 1 } },
            "systems": { "nes": { "values": { "GAMMA_OUT": 1.2 } } }
          }
        }"#,
    )
}

fn presets(dir: &Path) -> Presets {
    Presets::with(&dir.join("forma/presets.json"), vec![sample()])
}

#[test]
fn forma_ships_a_preset_for_a_pi_5_on_a_monitor() {
    let all = Presets::built_in();
    let pi = all.iter().find(|p| p.id == "rpi5-monitor").unwrap();
    assert!(pi.name.contains("Pi 5"));
    let shader = pi.shader.as_ref().unwrap();
    assert!(shader.enabled);
    assert_eq!(
        shader.global.preset.as_deref(),
        Some("Presets-ng/Monitor-Balanced")
    );
    // The costliest parts off on the Pi's GPU; a flat monitor needs no frame.
    for key in ["DO_BEZEL", "DO_AMBILIGHT", "DO_BLOOM", "DO_CURVATURE"] {
        assert_eq!(shader.global.values.get(key), Some(&0.0), "{key}");
    }
    assert_eq!(pi.options.get(None, "rewind"), Some(("0", "global")));
    assert_eq!(
        pi.options.get(Some("nds"), "shaderset"),
        Some(("none", "system"))
    );
    // Every id once.
    let mut ids: Vec<_> = all.iter().map(|p| p.id.as_str()).collect();
    ids.dedup();
    assert_eq!(ids.len(), all.len());
}

#[test]
fn one_preset_is_active_or_none() {
    let dir = tempfile::tempdir().unwrap();
    let p = presets(dir.path());
    assert_eq!(p.active(), None);
    assert!(!p.list()[0].active);

    p.set_active(Some("sample")).unwrap();
    assert_eq!(p.active().unwrap().id, "sample");
    let listed = &p.list()[0];
    assert!(listed.active && listed.shader);
    assert_eq!(
        listed.options,
        [
            "global.ratio = 4/3",
            "global.rewind = 0",
            "nes.ratio = 16/9",
            "snes.shaderset = none"
        ]
    );

    assert!(p.set_active(Some("nope")).is_err());
    assert_eq!(p.active().unwrap().id, "sample");
    p.set_active(None).unwrap();
    assert_eq!(p.active(), None);
}

#[test]
fn a_launch_gets_the_presets_options_where_the_user_set_nothing_before_them() {
    let dir = tempfile::tempdir().unwrap();
    let conf_file = dir.path().join("batocera.conf");
    let p = presets(dir.path());
    std::fs::write(&conf_file, "global.ratio=1/1\nglobal.smooth=1\n").unwrap();

    // None active: nothing.
    p.before_launch(&conf_file, "nes", "Mario.nes").unwrap();
    assert_eq!(
        std::fs::read_to_string(&conf_file).unwrap(),
        "global.ratio=1/1\nglobal.smooth=1\n"
    );

    p.set_active(Some("sample")).unwrap();
    p.before_launch(&conf_file, "nes", "Mario.nes").unwrap();
    let set = conf::read(&conf_file);
    // The preset's NES ratio is nearer than the user's global one; its global rewind applies
    // where the user has none.
    assert_eq!(set.get("nes.ratio").map(String::as_str), Some("16/9"));
    assert_eq!(set.get("global.rewind").map(String::as_str), Some("0"));
    assert_eq!(set.get("global.ratio").map(String::as_str), Some("1/1"));
    // Another system's are not this launch's.
    assert!(!set.contains_key("snes.shaderset"));

    // After the game, the file is the user's again.
    conf::clear_for_launch(&conf_file).unwrap();
    assert_eq!(
        std::fs::read_to_string(&conf_file).unwrap(),
        "global.ratio=1/1\nglobal.smooth=1\n"
    );

    // The user's own value wins over the preset's: the system's, the game's, or (for one the
    // preset gives every system) the global one.
    std::fs::write(
        &conf_file,
        "nes.ratio=core\nglobal.rewind=1\nnes[\"Mario.nes\"].shaderset=retro\n",
    )
    .unwrap();
    p.before_launch(&conf_file, "nes", "Mario.nes").unwrap();
    assert!(
        !std::fs::read_to_string(&conf_file)
            .unwrap()
            .contains("# forma:")
    );
    std::fs::write(&conf_file, "snes[\"Zelda.sfc\"].shaderset=retro\n").unwrap();
    p.before_launch(&conf_file, "snes", "Zelda.sfc").unwrap();
    let set = conf::read(&conf_file);
    assert!(!set.contains_key("snes.shaderset"));
    assert_eq!(set.get("global.ratio").map(String::as_str), Some("4/3"));
}
