//! RetroAchievements, read-only: the user's profile and a game's progress.
//!
//! Unlocking stays inside RetroArch, which Batocera already logs in from `batocera.conf`. Forma
//! only shows. The Web API needs the user's **web API key** (retroachievements.org → Settings →
//! Keys), which Batocera does not store: it is the Forma setting `ra.apiKey`. The username
//! defaults to Batocera's.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{Cache, Fetched, Http, batocera_conf, fetch, lenient};
use crate::error::{Error, Result};
use crate::library::Library;
use crate::paths::Paths;

pub const USERNAME_SETTING: &str = "ra.username";
pub const API_KEY_SETTING: &str = "ra.apiKey";

const API: &str = "https://retroachievements.org/API";
const MEDIA: &str = "https://media.retroachievements.org";
const MAX_AGE: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub struct Account {
    pub username: String,
    api_key: String,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

/// The account from Forma's settings, falling back to Batocera's username.
pub fn account(lib: &Library, paths: &Paths) -> Result<Account> {
    let username = lib
        .setting_text(USERNAME_SETTING)?
        .or_else(|| {
            batocera_conf(&paths.batocera_conf()).remove("global.retroachievements.username")
        })
        .filter(|u| !u.is_empty())
        .ok_or_else(|| {
            Error::NotConfigured(
                "no RetroAchievements username: log in under Batocera's RetroAchievements \
                 settings, or set ra.username"
                    .into(),
            )
        })?;
    let api_key = lib
        .setting_text(API_KEY_SETTING)?
        .filter(|k| !k.is_empty())
        .ok_or_else(|| {
            Error::NotConfigured(
                "set your RetroAchievements web API key (retroachievements.org → Settings → \
                 Keys) as the Forma setting ra.apiKey"
                    .into(),
            )
        })?;
    Ok(Account { username, api_key })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaUser {
    pub username: String,
    pub points: i64,
    pub softcore_points: i64,
    pub true_points: i64,
    pub rank: Option<i64>,
    pub member_since: Option<String>,
    pub rich_presence: Option<String>,
    pub avatar: Option<String>,
    pub recently_played: Vec<RaRecent>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaRecent {
    pub ra_game_id: i64,
    pub title: String,
    pub console: String,
    pub icon: Option<String>,
    pub last_played: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaGame {
    pub ra_game_id: i64,
    pub title: String,
    pub console: String,
    pub icon: Option<String>,
    pub achievement_count: i64,
    pub earned: i64,
    pub earned_hardcore: i64,
    /// 0.0–1.0.
    pub completion: f64,
    pub achievements: Vec<RaAchievement>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaAchievement {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub points: i64,
    pub badge: Option<String>,
    /// When earned (softcore or hardcore), as RetroAchievements reports it.
    pub earned_at: Option<String>,
    pub earned_hardcore_at: Option<String>,
}

/// Profile and recently played games.
pub fn user(
    http: &dyn Http,
    cache: &Cache,
    account: &Account,
    media: &str,
) -> Result<Fetched<RaUser>> {
    let url = format!(
        "{API}/API_GetUserSummary.php?u={}&y={}&g=10&a=0",
        encode(&account.username),
        encode(&account.api_key)
    );
    fetch(
        http,
        cache,
        &format!("ra-user-{}", account.username),
        &url,
        MAX_AGE,
        |body| {
            let raw: UserRaw = serde_json::from_slice(body).map_err(|e| e.to_string())?;
            Ok(RaUser {
                username: raw.user.ok_or("no User in the answer")?,
                points: raw.total_points.unwrap_or(0),
                softcore_points: raw.total_softcore_points.unwrap_or(0),
                true_points: raw.total_true_points.unwrap_or(0),
                rank: raw.rank,
                member_since: raw.member_since,
                rich_presence: raw.rich_presence_msg.filter(|m| !m.is_empty()),
                avatar: raw.user_pic.map(|p| media_url(media, &p)),
                recently_played: raw
                    .recently_played
                    .into_iter()
                    .filter_map(|g| {
                        Some(RaRecent {
                            ra_game_id: g.game_id?,
                            title: g.title.unwrap_or_default(),
                            console: g.console_name.unwrap_or_default(),
                            icon: g.image_icon.map(|p| media_url(media, &p)),
                            last_played: g.last_played,
                        })
                    })
                    .collect(),
            })
        },
    )
}

/// A game's achievements and the user's progress in it.
pub fn game(
    http: &dyn Http,
    cache: &Cache,
    account: &Account,
    ra_game_id: i64,
    media: &str,
) -> Result<Fetched<RaGame>> {
    let url = format!(
        "{API}/API_GetGameInfoAndUserProgress.php?g={ra_game_id}&u={}&y={}",
        encode(&account.username),
        encode(&account.api_key)
    );
    let key = format!("ra-game-{ra_game_id}-{}", account.username);
    fetch(http, cache, &key, &url, MAX_AGE, |body| {
        let raw: GameRaw = serde_json::from_slice(body).map_err(|e| e.to_string())?;
        let mut achievements: Vec<(i64, RaAchievement)> = raw
            .achievements
            .into_values()
            .filter_map(|a| {
                Some((
                    a.display_order.unwrap_or(0),
                    RaAchievement {
                        id: a.id?,
                        title: a.title.unwrap_or_default(),
                        description: a.description.unwrap_or_default(),
                        points: a.points.unwrap_or(0),
                        badge: a
                            .badge_name
                            .map(|b| media_url(media, &format!("/Badge/{b}.png"))),
                        earned_at: a.date_earned,
                        earned_hardcore_at: a.date_earned_hardcore,
                    },
                ))
            })
            .collect();
        achievements.sort_by_key(|(order, a)| (*order, a.id));
        let achievements: Vec<RaAchievement> = achievements.into_iter().map(|(_, a)| a).collect();
        let count = raw.num_achievements.unwrap_or(achievements.len() as i64);
        Ok(RaGame {
            ra_game_id: raw.id.ok_or("no ID in the answer")?,
            title: raw.title.unwrap_or_default(),
            console: raw.console_name.unwrap_or_default(),
            icon: raw.image_icon.map(|p| media_url(media, &p)),
            achievement_count: count,
            earned: raw.num_awarded_to_user.unwrap_or(0),
            earned_hardcore: raw.num_awarded_to_user_hardcore.unwrap_or(0),
            completion: raw.user_completion.map_or(0.0, |c| c / 100.0),
            achievements,
        })
    })
}

/// A file from RetroAchievements' media server (badges, icons, avatars), cached on disk so it
/// is fetched once and shown offline afterwards. `path` is what follows the host, e.g.
/// `Badge/04913.png`; anything else is refused.
pub fn media_file(http: &dyn Http, dir: &Path, path: &str) -> Result<PathBuf> {
    let valid = !path.is_empty()
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != ".."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        });
    if !valid {
        return Err(Error::Network(format!(
            "not a RetroAchievements media path: {path}"
        )));
    }
    let file = dir.join(path);
    if file.is_file() {
        return Ok(file);
    }
    let body = http
        .get(&format!("{MEDIA}/{path}"))
        .map_err(|e| Error::Network(format!("media.retroachievements.org: {}", e.message)))?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::write(&file, body).map_err(|e| Error::io(&file, e))?;
    Ok(file)
}

/// `/Badge/1.png` under the caller's media prefix (in the app, its own `forma://` proxy).
fn media_url(prefix: &str, path: &str) -> String {
    format!("{prefix}{}", path.trim_start_matches('/'))
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ---- The API's own shapes. Lenient: fields go missing and numbers arrive as strings. ------

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UserRaw {
    user: Option<String>,
    member_since: Option<String>,
    rich_presence_msg: Option<String>,
    #[serde(default, deserialize_with = "lenient::int")]
    total_points: Option<i64>,
    #[serde(default, deserialize_with = "lenient::int")]
    total_softcore_points: Option<i64>,
    #[serde(default, deserialize_with = "lenient::int")]
    total_true_points: Option<i64>,
    #[serde(default, deserialize_with = "lenient::int")]
    rank: Option<i64>,
    user_pic: Option<String>,
    #[serde(default)]
    recently_played: Vec<RecentRaw>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RecentRaw {
    #[serde(rename = "GameID", default, deserialize_with = "lenient::int")]
    game_id: Option<i64>,
    title: Option<String>,
    console_name: Option<String>,
    image_icon: Option<String>,
    last_played: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct GameRaw {
    #[serde(rename = "ID", default, deserialize_with = "lenient::int")]
    id: Option<i64>,
    title: Option<String>,
    console_name: Option<String>,
    image_icon: Option<String>,
    #[serde(default, deserialize_with = "lenient::int")]
    num_achievements: Option<i64>,
    #[serde(default, deserialize_with = "lenient::int")]
    num_awarded_to_user: Option<i64>,
    #[serde(default, deserialize_with = "lenient::int")]
    num_awarded_to_user_hardcore: Option<i64>,
    #[serde(default, deserialize_with = "lenient::float")]
    user_completion: Option<f64>,
    #[serde(default, deserialize_with = "map_or_empty_list")]
    achievements: HashMap<String, AchievementRaw>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AchievementRaw {
    #[serde(rename = "ID", default, deserialize_with = "lenient::int")]
    id: Option<i64>,
    title: Option<String>,
    description: Option<String>,
    #[serde(default, deserialize_with = "lenient::int")]
    points: Option<i64>,
    badge_name: Option<String>,
    #[serde(default, deserialize_with = "lenient::int")]
    display_order: Option<i64>,
    date_earned: Option<String>,
    date_earned_hardcore: Option<String>,
}

/// PHP encodes an empty map as `[]`.
fn map_or_empty_list<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<HashMap<String, AchievementRaw>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MapOrList {
        Map(HashMap<String, AchievementRaw>),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Option::<MapOrList>::deserialize(d)? {
        Some(MapOrList::Map(m)) => m,
        _ => HashMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::fake::FakeHttp;
    use crate::test_support::fixture_paths;

    const PREFIX: &str = "forma://localhost/ra-media/";

    fn configured() -> (Library, Account) {
        let mut lib = Library::open_in_memory().unwrap();
        // As the SDK stores it: a JSON string.
        lib.set_setting(API_KEY_SETTING, Some("\"k3y&/\"")).unwrap();
        let account = account(&lib, &fixture_paths()).unwrap();
        (lib, account)
    }

    fn http() -> FakeHttp {
        FakeHttp::default()
            .fixture("API_GetUserSummary", "ra-user.json")
            .fixture("API_GetGameInfoAndUserProgress", "ra-game.json")
    }

    #[test]
    fn the_account_comes_from_settings_and_batocera() {
        let (_, account) = configured();
        assert_eq!(account.username, "fixture-user", "from batocera.conf");
        assert_eq!(account.api_key, "k3y&/");
        assert!(
            !format!("{account:?}").contains("k3y"),
            "Debug never shows the key"
        );

        let lib = Library::open_in_memory().unwrap();
        let err = super::account(&lib, &fixture_paths()).unwrap_err();
        assert!(matches!(err, Error::NotConfigured(_)));
        assert!(err.to_string().contains("ra.apiKey"));
    }

    #[test]
    fn reads_the_user_summary() {
        let dir = tempfile::tempdir().unwrap();
        let (_, account) = configured();
        let http = http();
        let user = user(&http, &Cache::new(dir.path().into()), &account, PREFIX)
            .unwrap()
            .data;
        assert_eq!(user.username, "fixture-user");
        assert_eq!(
            (user.points, user.true_points, user.rank),
            (4210, 9875, Some(15342))
        );
        assert_eq!(
            user.avatar.as_deref(),
            Some("forma://localhost/ra-media/UserPic/fixture-user.png")
        );
        assert_eq!(user.recently_played.len(), 2);
        assert_eq!(user.recently_played[0].ra_game_id, 1446, "a string GameID");
        assert_eq!(user.recently_played[1].ra_game_id, 1, "a numeric GameID");

        let url = http.calls.lock().unwrap()[0].clone();
        assert!(
            url.contains("u=fixture-user&y=k3y%26%2F"),
            "the key is URL-encoded: {url}"
        );
    }

    #[test]
    fn reads_game_progress_in_display_order() {
        let dir = tempfile::tempdir().unwrap();
        let (_, account) = configured();
        let game = game(
            &http(),
            &Cache::new(dir.path().into()),
            &account,
            1446,
            PREFIX,
        )
        .unwrap()
        .data;
        assert_eq!(game.title, "Super Mario Bros.");
        assert_eq!(
            (game.achievement_count, game.earned, game.earned_hardcore),
            (3, 2, 1)
        );
        assert!((game.completion - 0.6667).abs() < 1e-9);
        let titles: Vec<&str> = game.achievements.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Mushroom Kingdom", "Warp Zone", "Princess Toadstool"]
        );
        assert_eq!(game.achievements[1].points, 10, "a string Points");
        assert_eq!(
            game.achievements[0].badge.as_deref(),
            Some("forma://localhost/ra-media/Badge/04913.png")
        );
        assert!(game.achievements[2].earned_at.is_none());
    }

    #[test]
    fn a_game_without_achievements_is_empty_not_an_error() {
        let raw: GameRaw =
            serde_json::from_str(r#"{ "ID": 9, "Title": "x", "Achievements": [] }"#).unwrap();
        assert!(raw.achievements.is_empty());
    }

    #[test]
    fn media_is_fetched_once_and_only_from_safe_paths() {
        let dir = tempfile::tempdir().unwrap();
        let mut http = FakeHttp::default();
        http.routes.insert("Badge/04913.png", Ok(b"PNG".to_vec()));
        let file = media_file(&http, dir.path(), "Badge/04913.png").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"PNG");
        media_file(&http, dir.path(), "Badge/04913.png").unwrap();
        assert_eq!(http.calls(), 1, "the second read came from disk");

        for bad in [
            "../etc/passwd",
            "Badge/../../x",
            "/abs",
            "",
            "Badge//x",
            "a b.png",
            "x?y=1",
        ] {
            assert!(media_file(&http, dir.path(), bad).is_err(), "{bad}");
        }
    }
}
