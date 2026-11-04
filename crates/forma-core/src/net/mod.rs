//! Talking to services: RetroAchievements and the libretro netplay lobby.
//!
//! Everything goes through [`Http`], so tests use a fake and never touch the network. Answers
//! are cached on disk; offline, the last good answer is served marked `stale`, so a sofa UI
//! shows yesterday's achievements rather than an error.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::error::{Error, Result};

pub mod achievements;
pub mod netplay;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    /// The HTTP status, when the server answered at all.
    pub status: Option<u16>,
    pub message: String,
}

/// A blocking GET. Call it off the UI thread.
pub trait Http: Send + Sync {
    fn get(&self, url: &str) -> std::result::Result<Vec<u8>, HttpError>;

    /// A download larger than an API answer may be (a theme package), up to `limit` bytes.
    fn download(&self, url: &str, limit: u64) -> std::result::Result<Vec<u8>, HttpError> {
        let _ = limit;
        self.get(url)
    }
}

/// The real client: rustls with bundled root certificates, so it works on a Batocera image
/// whatever certificates the system has.
pub struct UreqHttp {
    agent: ureq::Agent,
}

/// Larger answers are refused: no API we call comes close.
const MAX_BODY: u64 = 16 * 1024 * 1024;

impl Default for UreqHttp {
    fn default() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .user_agent(concat!("Forma/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self { agent }
    }
}

impl Http for UreqHttp {
    fn get(&self, url: &str) -> std::result::Result<Vec<u8>, HttpError> {
        self.download(url, MAX_BODY)
    }

    fn download(&self, url: &str, limit: u64) -> std::result::Result<Vec<u8>, HttpError> {
        // An API answer gets the agent's 15 s; a package on slow Wi-Fi gets minutes.
        let timeout = if limit > MAX_BODY {
            Duration::from_secs(600)
        } else {
            Duration::from_secs(15)
        };
        let mut response = self
            .agent
            .get(url)
            .config()
            .timeout_global(Some(timeout))
            .build()
            .call()
            .map_err(|e| HttpError {
                status: match &e {
                    ureq::Error::StatusCode(code) => Some(*code),
                    _ => None,
                },
                message: e.to_string(),
            })?;
        response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|e| HttpError {
                status: None,
                message: e.to_string(),
            })
    }
}

/// An answer, and whether it came from the network just now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fetched<T> {
    pub data: T,
    /// Served from the cache because the service could not be reached.
    pub stale: bool,
    /// When the data was fetched, ISO 8601 UTC.
    pub fetched_at: String,
}

/// Answers kept on disk, one file per request.
#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn file(&self, key: &str) -> PathBuf {
        let safe: String = key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{safe}.json"))
    }

    fn get(&self, key: &str) -> Option<(Vec<u8>, SystemTime)> {
        let file = self.file(key);
        let modified = std::fs::metadata(&file).ok()?.modified().ok()?;
        Some((std::fs::read(file).ok()?, modified))
    }

    fn put(&self, key: &str, body: &[u8]) {
        // A cache that cannot be written only costs a refetch.
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = std::fs::write(self.file(key), body);
    }
}

/// Fresh from the cache if young enough, else from the network (cached if it parses), else
/// the last good answer marked stale.
pub(crate) fn fetch<T>(
    http: &dyn Http,
    cache: &Cache,
    key: &str,
    url: &str,
    max_age: Duration,
    parse: impl Fn(&[u8]) -> std::result::Result<T, String>,
) -> Result<Fetched<T>> {
    let now = SystemTime::now();
    let cached = cache.get(key);
    if let Some((body, at)) = &cached
        && now.duration_since(*at).unwrap_or(Duration::MAX) < max_age
        && let Ok(data) = parse(body)
    {
        return Ok(Fetched {
            data,
            stale: false,
            fetched_at: crate::time::iso_utc(*at),
        });
    }

    let failure = match http.get(url) {
        Ok(body) => match parse(&body) {
            Ok(data) => {
                cache.put(key, &body);
                return Ok(Fetched {
                    data,
                    stale: false,
                    fetched_at: crate::time::iso_utc(now),
                });
            }
            Err(e) => Error::Network(format!("unexpected answer from {}: {e}", host(url))),
        },
        Err(HttpError {
            status: Some(401 | 403),
            ..
        }) => {
            // Credentials are wrong: a stale answer would hide that.
            return Err(Error::NotConfigured(format!(
                "{} refused the credentials",
                host(url)
            )));
        }
        Err(e) => Error::Network(format!("{} unreachable: {}", host(url), e.message)),
    };
    match cached.and_then(|(body, at)| parse(&body).ok().map(|d| (d, at))) {
        Some((data, at)) => Ok(Fetched {
            data,
            stale: true,
            fetched_at: crate::time::iso_utc(at),
        }),
        None => Err(failure),
    }
}

/// The host part of a URL, for messages that must not echo an API key.
fn host(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("the service")
}

/// Values from `batocera.conf` (`key=value` lines; `#` comments).
pub fn batocera_conf(path: &Path) -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
}

/// Deserialises a number that a service sometimes sends as a string.
pub(crate) mod lenient {
    use serde::{Deserialize, Deserializer};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrText {
        Num(serde_json::Number),
        Text(String),
    }

    fn parse(v: NumOrText) -> Option<f64> {
        match v {
            NumOrText::Num(n) => n.as_f64(),
            NumOrText::Text(s) => s.trim().trim_end_matches('%').parse().ok(),
        }
    }

    pub fn int<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
        Ok(Option::<NumOrText>::deserialize(d)?
            .and_then(parse)
            .map(|f| f as i64))
    }

    pub fn float<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
        Ok(Option::<NumOrText>::deserialize(d)?.and_then(parse))
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::{Http, HttpError};

    /// Answers by URL substring, and remembers what was asked.
    #[derive(Default)]
    pub struct FakeHttp {
        pub routes: HashMap<&'static str, Result<Vec<u8>, HttpError>>,
        pub calls: Mutex<Vec<String>>,
    }

    impl FakeHttp {
        pub fn fixture(mut self, needle: &'static str, file: &str) -> Self {
            let path = format!("{}/../../fixtures/http/{file}", env!("CARGO_MANIFEST_DIR"));
            self.routes.insert(needle, Ok(std::fs::read(path).unwrap()));
            self
        }

        pub fn fail(mut self, needle: &'static str, status: Option<u16>) -> Self {
            let message = format!("fake failure {status:?}");
            self.routes
                .insert(needle, Err(HttpError { status, message }));
            self
        }

        pub fn calls(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    impl Http for FakeHttp {
        fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
            self.calls.lock().unwrap().push(url.to_owned());
            self.routes
                .iter()
                .find(|(needle, _)| url.contains(*needle))
                .map(|(_, r)| r.clone())
                .unwrap_or_else(|| {
                    Err(HttpError {
                        status: None,
                        message: format!("no route for {url}"),
                    })
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeHttp;
    use super::*;

    fn parse_num(body: &[u8]) -> std::result::Result<u32, String> {
        std::str::from_utf8(body)
            .map_err(|e| e.to_string())?
            .trim()
            .parse()
            .map_err(|e| format!("{e}"))
    }

    fn answering(body: &str) -> FakeHttp {
        let mut http = FakeHttp::default();
        http.routes.insert("example", Ok(body.as_bytes().to_vec()));
        http
    }

    #[test]
    fn caches_a_good_answer_and_reuses_it_while_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        let http = answering("7");
        let hour = Duration::from_secs(3600);
        let first = fetch(&http, &cache, "k", "https://example.org/x", hour, parse_num).unwrap();
        let again = fetch(&http, &cache, "k", "https://example.org/x", hour, parse_num).unwrap();
        assert_eq!((first.data, again.data), (7, 7));
        assert!(!again.stale);
        assert_eq!(http.calls(), 1);
    }

    #[test]
    fn serves_the_last_good_answer_when_offline_marked_stale() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        fetch(
            &answering("7"),
            &cache,
            "k",
            "https://example.org/x",
            Duration::ZERO,
            parse_num,
        )
        .unwrap();
        let offline = FakeHttp::default().fail("example", None);
        let answer = fetch(
            &offline,
            &cache,
            "k",
            "https://example.org/x",
            Duration::ZERO,
            parse_num,
        )
        .unwrap();
        assert_eq!(answer.data, 7);
        assert!(answer.stale);
    }

    #[test]
    fn never_caches_an_answer_that_does_not_parse() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        let err = fetch(
            &answering("<html>"),
            &cache,
            "k",
            "https://example.org/x",
            Duration::ZERO,
            parse_num,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Network(_)), "{err}");
        assert!(cache.get("k").is_none());
    }

    #[test]
    fn refused_credentials_are_an_error_even_with_a_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().to_path_buf());
        fetch(
            &answering("7"),
            &cache,
            "k",
            "https://example.org/x?y=SECRET",
            Duration::ZERO,
            parse_num,
        )
        .unwrap();
        let refused = FakeHttp::default().fail("example", Some(401));
        let err = fetch(
            &refused,
            &cache,
            "k",
            "https://example.org/x?y=SECRET",
            Duration::ZERO,
            parse_num,
        )
        .unwrap_err();
        assert!(matches!(err, Error::NotConfigured(_)));
        assert!(
            !err.to_string().contains("SECRET"),
            "messages never echo the key"
        );
    }

    #[test]
    fn reads_batocera_conf() {
        let conf = batocera_conf(&crate::test_support::fixture_paths().batocera_conf());
        assert_eq!(conf["global.retroachievements.username"], "fixture-user");
        assert_eq!(conf["global.netplay.port"], "55435");
        assert!(!conf.keys().any(|k| k.starts_with('#')));
    }

    #[test]
    fn cache_file_names_are_safe() {
        let cache = Cache::new(PathBuf::from("/c"));
        assert_eq!(
            cache.file("ra/user:../x"),
            Path::new("/c/ra_user_.._x.json")
        );
    }
}
