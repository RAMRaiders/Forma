use std::sync::{Arc, Mutex};

use super::*;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// Answers from `fixtures/batocera`, and records every command.
#[derive(Clone, Default)]
struct FakeShell {
    ran: Arc<Mutex<Vec<String>>>,
}

impl Shell for FakeShell {
    fn run(&self, program: &str, args: &[&str]) -> Result<String> {
        let line = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        self.ran.lock().unwrap().push(line);
        let file = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join("_");
        Ok(
            std::fs::read_to_string(fixtures().join("batocera").join(format!("{file}.txt")))
                .unwrap_or_default(),
        )
    }
}

fn settings(conf_text: &str) -> (SystemSettings, FakeShell, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let conf = dir.path().join("batocera.conf");
    std::fs::write(&conf, conf_text).unwrap();
    let shell = FakeShell::default();
    let s = SystemSettings::new(Box::new(shell.clone()), &conf).with_files(
        &fixtures().join("usr-share-zoneinfo"),
        &fixtures().join("usr-share-batocera/batocera.version"),
    );
    (s, shell, dir)
}

fn get<'a>(all: &'a [SystemSetting], key: &str) -> &'a SystemSetting {
    all.iter().find(|s| s.key == key).unwrap()
}

fn values(s: &SystemSetting) -> Vec<&str> {
    s.choices.iter().map(|c| c.value.as_str()).collect()
}

#[test]
fn info_is_the_version_and_the_lines_that_say_something() {
    let (s, _, _dir) = settings("");
    let info = s.info();
    let labels: Vec<&str> = info.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(info[0].value, "43.1 2026/05/29 17:30");
    assert!(labels.contains(&"Model") && labels.contains(&"Network IP Address"));
    assert!(!labels.contains(&"Display Output"), "empty");
    assert!(
        !labels.contains(&"Display Refresh Rate"),
        "nothing filled in"
    );
    assert_eq!(
        info.iter().find(|i| i.label == "Model").unwrap().value,
        "Raspberry Pi 5 Model B Rev 1 0"
    );
}

#[test]
fn choices_come_from_batoceras_tools() {
    let (s, _, _dir) = settings("system.timezone=Europe/Lisbon\naudio.device=auto\n");
    let all = s.get();
    assert_eq!(
        values(get(&all, "es.resolution")),
        ["max-1920x1080", "1920x1080.60000", "1280x720.60000"]
    );
    assert_eq!(
        values(get(&all, "global.videooutput")),
        ["HDMI-A-1", "HDMI-A-2"]
    );
    let audio = get(&all, "audio.device");
    assert_eq!(audio.value.as_deref(), Some("auto"));
    assert_eq!(audio.choices[0].label, "Automatic");
    assert_eq!(
        audio.choices[1].label,
        "Built-in Audio Digital Stereo (HDMI)"
    );
    // Region/City only: no aliases (`GB`) or the `posix` copy.
    assert_eq!(
        values(get(&all, "system.timezone")),
        [
            "UTC",
            "America/Argentina/Buenos_Aires",
            "America/New_York",
            "Europe/Lisbon",
            "Europe/Paris"
        ]
    );
    assert_eq!(get(&all, "system.kblayout").choices[2].label, "Portuguese");
    let storage = get(&all, "storage");
    assert_eq!(storage.value.as_deref(), Some("INTERNAL"));
    assert_eq!(
        storage.choices[2].value,
        "DEV 570aa8a5-b685-4bac-a259-d092c5b3de44"
    );
    assert_eq!(get(&all, "system.hostname").kind, "text");
    assert_eq!(get(&all, "es.resolution").applies, "reboot");
}

#[test]
fn setting_runs_batoceras_tool_then_saves_the_value() {
    let (s, shell, _dir) = settings("system.kbvariant=nodeadkeys\n");
    s.set("system.timezone", Some("Europe/Paris")).unwrap();
    s.set("audio.device", Some("alsa_output.hdmi-stereo"))
        .unwrap();
    s.set("system.kblayout", Some("fr")).unwrap();
    s.set("display.rotate", Some("2")).unwrap();
    s.set("audio.volume", Some("75")).unwrap();
    s.set("storage", Some("DEV 570aa8a5-b685-4bac-a259-d092c5b3de44"))
        .unwrap();
    let ran = shell.ran.lock().unwrap().clone();
    let writes: Vec<&str> = ran
        .iter()
        .map(String::as_str)
        .filter(|l| !l.contains(" list") && !l.ends_with("listModes"))
        .collect();
    assert_eq!(
        writes,
        [
            "batocera-timezone set Europe/Paris",
            "batocera-settings-set system.timezone Europe/Paris",
            "batocera-audio set alsa_output.hdmi-stereo",
            "batocera-settings-set audio.device alsa_output.hdmi-stereo",
            "batocera-keyboard set fr nodeadkeys",
            "batocera-settings-set system.kblayout fr",
            "batocera-settings-set display.rotate 2",
            "batocera-audio setSystemVolume 75",
            "batocera-settings-set audio.volume 75",
            "batocera-config storage DEV 570aa8a5-b685-4bac-a259-d092c5b3de44",
        ]
    );
}

#[test]
fn values_are_checked_and_unsetting_comments_the_key_out() {
    let (s, _, dir) = settings("es.resolution=1280x720.60000\n");
    assert!(s.set("es.resolution", Some("800x600.60000")).is_err());
    assert!(s.set("system.timezone", Some("Mars/Olympus")).is_err());
    assert!(s.set("system.hostname", Some("my box")).is_err());
    assert!(s.set("system.hostname", Some("-box")).is_err());
    assert!(s.set("system.password", Some("x")).is_err());
    assert!(s.set("storage", None).is_err());
    assert!(s.set("audio.volume", Some("101")).is_err());
    assert!(s.set("audio.volume", Some("-5")).is_err());
    s.set("system.hostname", Some("retro-box")).unwrap();
    s.set("es.resolution", None).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("batocera.conf")).unwrap(),
        "#es.resolution=1280x720.60000\n"
    );
}

#[test]
fn the_volume_is_a_number_from_batocera_conf() {
    let (s, _, _dir) = settings("audio.volume=60\naudio.bgmusic=1\n");
    let all = s.get();
    let volume = get(&all, "audio.volume");
    assert_eq!(volume.kind, "number");
    assert_eq!(
        (volume.min, volume.max, volume.step),
        (Some(0), Some(100), Some(5))
    );
    assert_eq!(volume.value.as_deref(), Some("60"));
    assert_eq!(values(get(&all, "audio.bgmusic")), ["1", "0"]);
}
