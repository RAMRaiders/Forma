//! Batocera's own settings that Forma's menus change: RetroAchievements and netplay, the keys
//! EmulationStation's menus write in `batocera.conf`.
//!
//! Only the keys listed here can be read or written, each checked against its kind. Writes go
//! through `batocera-settings-set`, as ES's do, so ES, configgen and RetroArch see the same values.
//! A secret (the RetroAchievements password and token) is never read back: a theme only learns
//! whether it is set.

use std::path::Path;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::host::Shell;
use crate::net::{Http, batocera_conf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `0` or `1`.
    Bool,
    Text,
    Port,
    Secret,
    Choice(&'static [&'static str]),
    /// A RetroAchievements unlock sound: a file in `/usr/share/libretro/assets/sounds`, or `none`.
    Sound,
}

const SOUNDS_DIR: &str = "/usr/share/libretro/assets/sounds";

/// Batocera's netplay relays, as ES offers them (RetroArch's `netplay_mitm_server` ids).
const RELAYS: &[&str] = &["none", "nyc", "madrid", "montreal", "saopaulo", "custom"];

const KEYS: &[(&str, Kind)] = &[
    ("global.retroachievements", Kind::Bool),
    ("global.retroachievements.username", Kind::Text),
    ("global.retroachievements.password", Kind::Secret),
    ("global.retroachievements.token", Kind::Secret),
    ("global.retroachievements.hardcore", Kind::Bool),
    ("global.retroachievements.encore", Kind::Bool),
    ("global.retroachievements.leaderboards", Kind::Bool),
    ("global.retroachievements.richpresence", Kind::Bool),
    ("global.retroachievements.verbose", Kind::Bool),
    ("global.retroachievements.screenshot", Kind::Bool),
    ("global.retroachievements.challenge_indicators", Kind::Bool),
    ("global.retroachievements.unofficial", Kind::Bool),
    ("global.retroachievements.sound", Kind::Sound),
    ("global.netplay", Kind::Bool),
    ("global.netplay.nickname", Kind::Text),
    ("global.netplay.port", Kind::Port),
    ("global.netplay.relay", Kind::Choice(RELAYS)),
    ("global.netplay.customserver", Kind::Text),
];

/// One setting as a theme sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Setting {
    pub key: String,
    /// `None` for a secret, and for a key that is not set.
    pub value: Option<String>,
    pub kind: &'static str,
    /// Secrets: whether one is saved.
    pub set: bool,
    /// The values a choice (or a sound) accepts.
    pub choices: Vec<String>,
}

pub struct BatoceraSettings {
    shell: Box<dyn Shell>,
    conf: std::path::PathBuf,
    sounds: std::path::PathBuf,
}

impl BatoceraSettings {
    pub fn new(shell: Box<dyn Shell>, conf: &Path) -> Self {
        Self {
            shell,
            conf: conf.to_path_buf(),
            sounds: SOUNDS_DIR.into(),
        }
    }

    /// For tests: where the unlock sounds are.
    pub fn with_sounds_dir(mut self, dir: &Path) -> Self {
        self.sounds = dir.to_path_buf();
        self
    }

    /// Every allowed setting starting with `prefix` (e.g. `global.netplay`), or all of them.
    pub fn get(&self, prefix: &str) -> Vec<Setting> {
        let conf = batocera_conf(&self.conf);
        KEYS.iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|&(key, kind)| {
                let stored = conf.get(key).filter(|v| !v.is_empty()).cloned();
                let secret = kind == Kind::Secret;
                Setting {
                    key: key.to_owned(),
                    set: stored.is_some(),
                    value: if secret { None } else { stored },
                    kind: match kind {
                        Kind::Bool => "bool",
                        Kind::Text => "text",
                        Kind::Port => "port",
                        Kind::Secret => "secret",
                        Kind::Choice(_) | Kind::Sound => "choice",
                    },
                    choices: self.choices(kind),
                }
            })
            .collect()
    }

    /// Sets one setting. An empty value clears it (Batocera has no "remove").
    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        let kind = kind_of(key)?;
        if kind == Kind::Secret {
            return Err(Error::Invalid(format!(
                "`{key}` is set by logging in, not directly"
            )));
        }
        let value = self.check(key, kind, value)?;
        self.write(&[(key, &value)])
    }

    /// Logs in to RetroAchievements as ES does, and saves the account with the session token
    /// the emulators use. A refused login saves nothing.
    pub fn ra_login(&self, http: &dyn Http, username: &str, password: &str) -> Result<String> {
        let username = username.trim();
        if username.is_empty() || password.is_empty() {
            return Err(Error::Invalid(
                "a RetroAchievements username and password are needed".into(),
            ));
        }
        let url = format!(
            "https://retroachievements.org/dorequest.php?r=login&u={}&p={}",
            encode(username),
            encode(password)
        );
        let body = http
            .get(&url)
            // The URL carries the password: never echo it.
            .map_err(|e| {
                Error::Network(format!(
                    "RetroAchievements could not be reached ({})",
                    e.status.map_or("no answer".to_owned(), |s| s.to_string())
                ))
            })?;
        let answer: LoginAnswer = serde_json::from_slice(&body).map_err(|_| {
            Error::Network("RetroAchievements answered something unreadable".into())
        })?;
        match answer {
            LoginAnswer {
                success: true,
                token: Some(token),
                user,
                ..
            } if !token.is_empty() => {
                let user = user.unwrap_or_else(|| username.to_owned());
                self.write(&[
                    ("global.retroachievements", "1"),
                    ("global.retroachievements.username", &user),
                    ("global.retroachievements.password", password),
                    ("global.retroachievements.token", &token),
                ])?;
                Ok(user)
            }
            LoginAnswer { error, .. } => Err(Error::Invalid(format!(
                "RetroAchievements refused the login: {}",
                error.unwrap_or_else(|| "check the username and password".into())
            ))),
        }
    }

    /// Forgets the RetroAchievements account and turns achievements off.
    pub fn ra_logout(&self) -> Result<()> {
        self.write(&[
            ("global.retroachievements", "0"),
            ("global.retroachievements.password", ""),
            ("global.retroachievements.token", ""),
        ])
    }

    fn write(&self, pairs: &[(&str, &str)]) -> Result<()> {
        let mut args = Vec::with_capacity(pairs.len() * 2);
        for (k, v) in pairs {
            args.push(*k);
            args.push(*v);
        }
        self.shell.run("batocera-settings-set", &args).map(drop)
    }

    fn check(&self, key: &str, kind: Kind, value: &str) -> Result<String> {
        let value = value.trim();
        if value.contains(['\n', '\r']) {
            return Err(Error::Invalid(format!("`{key}` must be on one line")));
        }
        let ok = match kind {
            Kind::Bool => matches!(value, "0" | "1"),
            Kind::Port => value.is_empty() || value.parse::<u16>().is_ok_and(|p| p >= 1024),
            Kind::Text | Kind::Secret => value.len() <= 200,
            Kind::Choice(_) | Kind::Sound => {
                value.is_empty() || self.choices(kind).iter().any(|c| c == value)
            }
        };
        if ok {
            Ok(value.to_owned())
        } else {
            Err(Error::Invalid(match kind {
                Kind::Bool => format!("`{key}` is 0 or 1"),
                Kind::Port => format!("`{key}` is a port from 1024 to 65535"),
                Kind::Choice(_) | Kind::Sound => {
                    format!("`{key}` is one of: {}", self.choices(kind).join(", "))
                }
                Kind::Text | Kind::Secret => format!("`{key}` is too long"),
            }))
        }
    }

    fn choices(&self, kind: Kind) -> Vec<String> {
        match kind {
            Kind::Choice(values) => values.iter().map(|v| (*v).to_owned()).collect(),
            Kind::Sound => {
                let mut sounds: Vec<String> = std::fs::read_dir(&self.sounds)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter_map(|e| {
                        let path = e.path();
                        (path.extension()? == "ogg")
                            .then(|| path.file_stem()?.to_str().map(str::to_owned))?
                    })
                    .collect();
                sounds.sort();
                sounds.insert(0, "none".into());
                sounds
            }
            _ => Vec::new(),
        }
    }
}

fn kind_of(key: &str) -> Result<Kind> {
    KEYS.iter()
        .find(|(k, _)| *k == key)
        .map(|&(_, kind)| kind)
        .ok_or_else(|| Error::Invalid(format!("`{key}` is not a setting Forma can change")))
}

#[derive(serde::Deserialize)]
struct LoginAnswer {
    #[serde(rename = "Success", default)]
    success: bool,
    #[serde(rename = "User")]
    user: Option<String>,
    #[serde(rename = "Token")]
    token: Option<String>,
    #[serde(rename = "Error")]
    error: Option<String>,
}

/// Percent-encoding for a query value: unreserved characters as they are, everything else
/// escaped byte by byte.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::net::HttpError;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<Vec<String>>>);

    impl Shell for &'static Recorder {
        fn run(&self, program: &str, args: &[&str]) -> Result<String> {
            assert_eq!(program, "batocera-settings-set");
            self.0
                .lock()
                .unwrap()
                .push(args.iter().map(|a| (*a).to_owned()).collect());
            Ok(String::new())
        }
    }

    struct Answer(&'static str);

    impl Http for Answer {
        fn get(&self, url: &str) -> std::result::Result<Vec<u8>, HttpError> {
            assert!(url.starts_with("https://retroachievements.org/dorequest.php?r=login&u="));
            Ok(self.0.as_bytes().to_vec())
        }
    }

    fn setup() -> (&'static Recorder, BatoceraSettings, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let conf = dir.path().join("batocera.conf");
        std::fs::write(
            &conf,
            "global.retroachievements=1\nglobal.retroachievements.username=crazyamon\n\
             global.retroachievements.password=hunter2\nglobal.netplay.relay=madrid\n\
             #global.netplay.port=55435\n",
        )
        .unwrap();
        let sounds = dir.path().join("sounds");
        std::fs::create_dir(&sounds).unwrap();
        for f in ["mario-1up.ogg", "zelda-secret.ogg", "readme.txt"] {
            std::fs::write(sounds.join(f), "").unwrap();
        }
        let rec: &'static Recorder = Box::leak(Box::default());
        let settings = BatoceraSettings::new(Box::new(rec), &conf).with_sounds_dir(&sounds);
        (rec, settings, dir)
    }

    fn find<'a>(all: &'a [Setting], key: &str) -> &'a Setting {
        all.iter().find(|s| s.key == key).unwrap()
    }

    #[test]
    fn reads_the_allowed_keys_and_never_a_secret() {
        let (_, settings, _dir) = setup();
        let all = settings.get("global.retroachievements");
        assert_eq!(
            find(&all, "global.retroachievements").value.as_deref(),
            Some("1")
        );
        let password = find(&all, "global.retroachievements.password");
        assert_eq!((password.value.as_deref(), password.set), (None, true));
        assert!(!find(&all, "global.retroachievements.token").set);
        assert_eq!(
            find(&all, "global.retroachievements.sound").choices,
            ["none", "mario-1up", "zelda-secret"]
        );
        let netplay = settings.get("global.netplay");
        assert_eq!(
            find(&netplay, "global.netplay.relay").value.as_deref(),
            Some("madrid")
        );
        assert_eq!(
            find(&netplay, "global.netplay.port").value,
            None,
            "commented out"
        );
        assert!(netplay.iter().all(|s| s.key.starts_with("global.netplay")));
    }

    #[test]
    fn writes_checked_values_through_batocera_settings_set() {
        let (rec, settings, _dir) = setup();
        settings.set("global.netplay.relay", "nyc").unwrap();
        settings.set("global.netplay.port", " 55436 ").unwrap();
        settings
            .set("global.retroachievements.sound", "zelda-secret")
            .unwrap();
        settings.set("global.netplay.nickname", "").unwrap();
        assert_eq!(
            *rec.0.lock().unwrap(),
            [
                vec!["global.netplay.relay", "nyc"],
                vec!["global.netplay.port", "55436"],
                vec!["global.retroachievements.sound", "zelda-secret"],
                vec!["global.netplay.nickname", ""],
            ]
        );
        for (key, value) in [
            ("global.netplay", "yes"),
            ("global.netplay.port", "80"),
            ("global.netplay.relay", "tokyo"),
            ("global.netplay.nickname", "two\nlines"),
            ("global.retroachievements.password", "hunter3"),
            ("system.hostname", "x"),
        ] {
            assert!(
                matches!(settings.set(key, value), Err(Error::Invalid(_))),
                "{key}={value:?}"
            );
        }
        assert_eq!(rec.0.lock().unwrap().len(), 4, "nothing else was written");
    }

    #[test]
    fn a_login_saves_the_account_and_its_token_in_one_write() {
        let (rec, settings, _dir) = setup();
        let ok = Answer(r#"{"Success":true,"User":"CrazyAmon","Token":"tok123","Score":5}"#);
        assert_eq!(
            settings.ra_login(&ok, "crazyamon", "p&ss w").unwrap(),
            "CrazyAmon"
        );
        assert_eq!(
            *rec.0.lock().unwrap(),
            [vec![
                "global.retroachievements",
                "1",
                "global.retroachievements.username",
                "CrazyAmon",
                "global.retroachievements.password",
                "p&ss w",
                "global.retroachievements.token",
                "tok123",
            ]]
        );
    }

    #[test]
    fn a_refused_login_saves_nothing_and_says_why() {
        let (rec, settings, _dir) = setup();
        let refused = Answer(
            r#"{"Success":false,"Error":"Invalid User/Password combination. Please try again"}"#,
        );
        let err = settings
            .ra_login(&refused, "crazyamon", "wrong")
            .unwrap_err();
        assert!(err.to_string().contains("Invalid User/Password"), "{err}");
        assert!(!err.to_string().contains("wrong"));
        assert!(matches!(
            settings.ra_login(&refused, " ", "x"),
            Err(Error::Invalid(_))
        ));
        assert!(rec.0.lock().unwrap().is_empty());
        settings.ra_logout().unwrap();
        assert_eq!(
            rec.0.lock().unwrap()[0][..2],
            ["global.retroachievements", "0"]
        );
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(encode("Az09-_.~"), "Az09-_.~");
    }
}
