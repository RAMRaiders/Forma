//! RetroAchievements, read-only: the user's profile and a game's progress.
//!
//! Unlocking stays inside RetroArch, which Batocera already logs in from `batocera.conf`. Forma
//! only shows. The Web API needs the user's **web API key** (retroachievements.org → Settings →
//! Keys), which Batocera does not store: it is the Forma setting `ra.apiKey`. The username
//! defaults to Batocera's.

use std::collections::{HashMap, HashSet};
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
const CONNECT: &str = "https://retroachievements.org/dorequest.php";
const MEDIA: &str = "https://media.retroachievements.org";
const MAX_AGE: Duration = Duration::from_secs(300);
/// RetroAchievements' list of a console's games and hashes changes when games are added: a
/// week is fresh enough, and each console is one request.
const HASHES_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone)]
pub struct Account {
    pub username: String,
    /// The web API key, for the user's profile and the dates things were earned.
    api_key: Option<String>,
    /// The session token Batocera signed in with: enough for a game's progress.
    token: Option<String>,
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
    let api_key = lib.setting_text(API_KEY_SETTING)?.filter(|k| !k.is_empty());
    let token = batocera_conf(&paths.batocera_conf())
        .remove("global.retroachievements.token")
        .filter(|t| !t.is_empty());
    if api_key.is_none() && token.is_none() {
        return Err(Error::NotConfigured(
            "sign in to RetroAchievements in Forma's menu, or set your web API key \
             (retroachievements.org, Settings, Keys) as the Forma setting ra.apiKey"
                .into(),
        ));
    }
    Ok(Account {
        username,
        api_key,
        token,
    })
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
        encode(account.api_key.as_deref().ok_or_else(|| {
            Error::NotConfigured(
                "a profile needs the web API key, the Forma setting ra.apiKey".into(),
            )
        })?)
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
    game_as_of(http, cache, account, ra_game_id, media, MAX_AGE)
}

/// What a play session earned: the achievements unlocked since it started.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaSession {
    /// Forma's game id.
    pub game_id: i64,
    pub game: RaGame,
    /// Earned during the session, in the order they were earned.
    pub unlocked: Vec<RaAchievement>,
    /// Their points.
    pub points: i64,
}

/// Unlocks happen in RetroArch, which tells RetroAchievements at once; Forma asks afterwards,
/// fresh (never from the cache), for what was earned since `since`. A minute of slack covers a
/// box clock a little off from RetroAchievements'. `None` when offline: an old answer cannot
/// say what was just earned.
pub fn session(
    http: &dyn Http,
    cache: &Cache,
    account: &Account,
    game_id: i64,
    ra_game_id: i64,
    media: &str,
    since: std::time::SystemTime,
) -> Result<Option<RaSession>> {
    // Only the web API says when something was earned.
    if account.api_key.is_none() {
        return Ok(None);
    }
    let fetched = game_as_of(http, cache, account, ra_game_id, media, Duration::ZERO)?;
    if fetched.stale {
        return Ok(None);
    }
    let from = crate::time::iso_utc(since - Duration::from_secs(60)).replace('T', " ");
    let earned = |a: &RaAchievement| {
        [&a.earned_at, &a.earned_hardcore_at]
            .into_iter()
            .flatten()
            .any(|at| at.as_str() >= from.as_str())
    };
    let mut unlocked: Vec<RaAchievement> = fetched
        .data
        .achievements
        .iter()
        .filter(|a| earned(a))
        .cloned()
        .collect();
    unlocked.sort_by(|a, b| a.earned_at.cmp(&b.earned_at));
    let points = unlocked.iter().map(|a| a.points).sum();
    Ok(Some(RaSession {
        game_id,
        game: fetched.data,
        unlocked,
        points,
    }))
}

fn game_as_of(
    http: &dyn Http,
    cache: &Cache,
    account: &Account,
    ra_game_id: i64,
    media: &str,
    max_age: Duration,
) -> Result<Fetched<RaGame>> {
    let Some(api_key) = &account.api_key else {
        return signed_in_game(http, cache, account, ra_game_id, media, max_age);
    };
    let url = format!(
        "{API}/API_GetGameInfoAndUserProgress.php?g={ra_game_id}&u={}&y={}",
        encode(&account.username),
        encode(api_key)
    );
    let key = format!("ra-game-{ra_game_id}-{}", account.username);
    fetch(http, cache, &key, &url, max_age, |body| {
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

/// A game's progress from the sign-in alone, read as the emulators read it: its achievements,
/// then which of them the user has, in softcore and hardcore. No dates: those stay empty.
fn signed_in_game(
    http: &dyn Http,
    cache: &Cache,
    account: &Account,
    ra_game_id: i64,
    media: &str,
    max_age: Duration,
) -> Result<Fetched<RaGame>> {
    let user = encode(&account.username);
    let auth = format!(
        "u={user}&t={}",
        encode(account.token.as_deref().unwrap_or_default())
    );
    let sets = fetch(
        http,
        cache,
        &format!("ra-sets-{ra_game_id}"),
        &format!("{CONNECT}?r=achievementsets&g={ra_game_id}&{auth}"),
        max_age,
        |body| serde_json::from_slice::<SetsRaw>(body).map_err(|e| e.to_string()),
    )?;
    let unlocks = |hardcore: u8| {
        fetch(
            http,
            cache,
            &format!("ra-unlocks-{ra_game_id}-{hardcore}-{user}"),
            &format!("{CONNECT}?r=unlocks&g={ra_game_id}&h={hardcore}&{auth}"),
            max_age,
            |body| serde_json::from_slice::<UnlocksRaw>(body).map_err(|e| e.to_string()),
        )
    };
    let (softcore, hardcore) = (unlocks(0)?, unlocks(1)?);
    let stale = sets.stale || softcore.stale || hardcore.stale;
    let fetched_at = sets.fetched_at;
    let earned = |list: &UnlocksRaw, id: i64| list.user_unlocks.contains(&id).then(String::new);
    let core = sets
        .data
        .sets
        .into_iter()
        .find(|s| s.kind.as_deref() == Some("core"));
    let achievements: Vec<RaAchievement> = core
        .map(|s| s.achievements)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a.flags == Some(OFFICIAL) && a.id != Some(UNKNOWN_EMULATOR))
        .filter_map(|a| {
            let id = a.id?;
            Some(RaAchievement {
                id,
                title: a.title.unwrap_or_default(),
                description: a.description.unwrap_or_default(),
                points: a.points.unwrap_or(0),
                badge: a
                    .badge_name
                    .map(|b| media_url(media, &format!("/Badge/{b}.png"))),
                earned_at: earned(&softcore.data, id),
                earned_hardcore_at: earned(&hardcore.data, id),
            })
        })
        .collect();
    let count = achievements.len() as i64;
    let has = achievements
        .iter()
        .filter(|a| a.earned_at.is_some() || a.earned_hardcore_at.is_some())
        .count() as i64;
    let in_hardcore = achievements
        .iter()
        .filter(|a| a.earned_hardcore_at.is_some())
        .count() as i64;
    Ok(Fetched {
        data: RaGame {
            ra_game_id: sets.data.game_id.unwrap_or(ra_game_id),
            title: sets.data.title.unwrap_or_default(),
            console: String::new(),
            icon: sets
                .data
                .image_icon_url
                .and_then(|u| u.find("/Images/").map(|at| media_url(media, &u[at..]))),
            achievement_count: count,
            earned: has,
            earned_hardcore: in_hardcore,
            completion: if count > 0 {
                has as f64 / count as f64
            } else {
                0.0
            },
            achievements,
        },
        stale,
        fetched_at,
    })
}

/// A published achievement, as opposed to one still being tested.
const OFFICIAL: i64 = 3;
/// What RetroAchievements adds to a client it does not recognise, to warn its player.
const UNKNOWN_EMULATOR: i64 = 101_000_001;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SetsRaw {
    #[serde(default, deserialize_with = "lenient::int")]
    game_id: Option<i64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    image_icon_url: Option<String>,
    #[serde(default)]
    sets: Vec<SetRaw>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SetRaw {
    #[serde(rename = "Type", default)]
    kind: Option<String>,
    #[serde(default)]
    achievements: Vec<SetAchievementRaw>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SetAchievementRaw {
    #[serde(rename = "ID", default, deserialize_with = "lenient::int")]
    id: Option<i64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, deserialize_with = "lenient::int")]
    points: Option<i64>,
    #[serde(default)]
    badge_name: Option<String>,
    #[serde(default, deserialize_with = "lenient::int")]
    flags: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UnlocksRaw {
    #[serde(default)]
    user_unlocks: Vec<i64>,
}

/// Every hash RetroAchievements knows for a console's games, lower case, to the RA game id:
/// what a computed hash is looked up in. Lists games with no achievements too.
pub fn hash_index(
    http: &dyn Http,
    cache: &Cache,
    console: u32,
) -> Result<Fetched<HashMap<String, i64>>> {
    let url = format!("{CONNECT}?r=hashlibrary&c={console}");
    fetch(
        http,
        cache,
        &format!("ra-hashlibrary-{console}"),
        &url,
        HASHES_MAX_AGE,
        |body| {
            let library: HashLibrary = serde_json::from_slice(body).map_err(|e| e.to_string())?;
            let Some(hashes) = library.list.as_object() else {
                return Ok(HashMap::new());
            };
            Ok(hashes
                .iter()
                .filter_map(|(hash, id)| {
                    let id = id.as_i64().or_else(|| id.as_str()?.parse().ok())?;
                    Some((hash.to_ascii_lowercase(), id))
                })
                .collect())
        },
    )
}

/// The games of a console that have achievements. RetroAchievements lists hashes for others
/// too, and a game with none is not worth showing as matched. Needs the sign-in.
pub fn games_with_achievements(
    http: &dyn Http,
    cache: &Cache,
    account: &Account,
    console: u32,
) -> Result<Fetched<HashSet<i64>>> {
    let token = account.token.as_deref().ok_or_else(|| {
        Error::NotConfigured("sign in to RetroAchievements to match games".into())
    })?;
    let url = format!(
        "{CONNECT}?r=allprogress&c={console}&u={}&t={}",
        encode(&account.username),
        encode(token)
    );
    fetch(
        http,
        cache,
        &format!("ra-with-achievements-{console}"),
        &url,
        HASHES_MAX_AGE,
        |body| {
            let progress: AllProgress = serde_json::from_slice(body).map_err(|e| e.to_string())?;
            let Some(games) = progress.response.as_object() else {
                return Ok(HashSet::new());
            };
            Ok(games
                .iter()
                .filter(|(_, game)| game["Achievements"].as_i64().unwrap_or(0) > 0)
                .filter_map(|(id, _)| id.parse().ok())
                .collect())
        },
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AllProgress {
    /// A map of games, or an empty list for a console with none.
    #[serde(default)]
    response: serde_json::Value,
}

#[derive(Deserialize)]
struct HashLibrary {
    /// A map of hashes, or an empty list for a console with none.
    #[serde(rename = "MD5List", default)]
    list: serde_json::Value,
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
    fn a_session_is_what_was_earned_since_it_started_and_is_never_cached() {
        let (_, account) = configured();
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().into());
        let http = http();
        let at = |text: &str| {
            // "2026-10-29 20:11:02" in the fixture.
            let (date, time) = text.split_once(' ').unwrap();
            let [y, m, d]: [i64; 3] = date
                .split('-')
                .map(|n| n.parse().unwrap())
                .collect::<Vec<_>>()
                .try_into()
                .unwrap();
            let [h, mi, s]: [u64; 3] = time
                .split(':')
                .map(|n| n.parse().unwrap())
                .collect::<Vec<_>>()
                .try_into()
                .unwrap();
            // Days from the civil date (Howard Hinnant's inverse).
            let y = if m <= 2 { y - 1 } else { y };
            let era = y.div_euclid(400);
            let yoe = y - era * 400;
            let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
            let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
            std::time::UNIX_EPOCH
                + Duration::from_secs(days as u64 * 86_400 + h * 3600 + mi * 60 + s)
        };
        let earned: Vec<String> = {
            let g = game(&http, &cache, &account, 1446, PREFIX).unwrap().data;
            g.achievements
                .iter()
                .filter_map(|a| a.earned_at.clone())
                .collect()
        };
        let first = earned.iter().min().unwrap().clone();
        let s = session(&http, &cache, &account, 7, 1446, PREFIX, at(&first))
            .unwrap()
            .unwrap();
        assert_eq!(
            s.unlocked.len(),
            earned.len(),
            "all earned at or after the start"
        );
        assert_eq!(s.game_id, 7);
        assert_eq!(s.points, s.unlocked.iter().map(|a| a.points).sum::<i64>());
        let later = at(earned.iter().max().unwrap()) + Duration::from_secs(120);
        let none = session(&http, &cache, &account, 7, 1446, PREFIX, later)
            .unwrap()
            .unwrap();
        assert!(none.unlocked.is_empty());
        assert_eq!(http.calls(), 3, "a session always asks again");
    }

    #[test]
    fn hashes_index_a_consoles_games_in_lower_case() {
        let dir = tempfile::tempdir().unwrap();
        let mut http = FakeHttp::default();
        http.routes.insert(
            "hashlibrary",
            Ok(br#"{"Success":true,"MD5List":{"811B027EAF99C2DEF7B933C5208636DE":1446,"a12d74c73a0481599a5d832361d168f4":"1"}}"#.to_vec()),
        );
        let index = hash_index(&http, &Cache::new(dir.path().into()), 7)
            .unwrap()
            .data;
        assert_eq!(index.len(), 2);
        assert_eq!(index["811b027eaf99c2def7b933c5208636de"], 1446);
        assert_eq!(index["a12d74c73a0481599a5d832361d168f4"], 1, "a string ID");
        let calls = http.calls.lock().unwrap();
        assert!(calls[0].contains("r=hashlibrary&c=7"));
        assert!(!calls[0].contains("y="), "no key is sent");
    }

    #[test]
    fn a_console_with_nothing_answers_with_empty_lists() {
        let account = Account {
            username: "me".into(),
            api_key: None,
            token: Some("t0k".into()),
        };
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().into());
        let mut http = FakeHttp::default();
        http.routes.insert(
            "hashlibrary",
            Ok(br#"{"Success":true,"MD5List":[]}"#.to_vec()),
        );
        http.routes.insert(
            "allprogress",
            Ok(br#"{"Success":true,"Response":[]}"#.to_vec()),
        );
        assert!(hash_index(&http, &cache, 30).unwrap().data.is_empty());
        let with = games_with_achievements(&http, &cache, &account, 30).unwrap();
        assert!(with.data.is_empty());
    }

    #[test]
    fn the_sign_in_alone_gives_a_games_progress() {
        let account = Account {
            username: "me".into(),
            api_key: None,
            token: Some("t0k".into()),
        };
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().into());
        let mut http = FakeHttp::default();
        http.routes.insert(
            "achievementsets",
            Ok(br#"{"Success":true,"GameId":1,"Title":"Sonic","ImageIconUrl":"https://media.retroachievements.org/Images/085573.png","Sets":[{"Type":"core","Achievements":[
                {"ID":101000001,"Title":"Warning: Unknown Emulator","Points":0,"BadgeName":"00000","Flags":3},
                {"ID":10,"Title":"Rings","Description":"Get them","Points":5,"BadgeName":"00010","Flags":3},
                {"ID":11,"Title":"Boss","Points":10,"BadgeName":"00011","Flags":3},
                {"ID":12,"Title":"Being tested","Points":1,"BadgeName":"00012","Flags":5}
            ]}]}"#.to_vec()),
        );
        http.routes.insert(
            "h=0",
            Ok(br#"{"Success":true,"UserUnlocks":[10]}"#.to_vec()),
        );
        http.routes
            .insert("h=1", Ok(br#"{"Success":true,"UserUnlocks":[]}"#.to_vec()));
        let game = game(&http, &cache, &account, 1, "ra/").unwrap().data;
        assert_eq!(
            (game.achievement_count, game.earned),
            (2, 1),
            "only real ones"
        );
        assert_eq!(game.completion, 0.5);
        assert_eq!(game.icon.as_deref(), Some("ra/Images/085573.png"));
        assert_eq!(
            game.achievements[0].badge.as_deref(),
            Some("ra/Badge/00010.png")
        );
        assert!(game.achievements[0].earned_at.is_some());
        assert!(game.achievements[1].earned_at.is_none());
        assert!(
            session(
                &http,
                &cache,
                &account,
                7,
                1,
                "ra/",
                std::time::SystemTime::now()
            )
            .unwrap()
            .is_none(),
            "no dates, so no session"
        );
    }

    #[test]
    fn the_account_comes_from_settings_and_batocera() {
        let (_, account) = configured();
        assert_eq!(account.username, "fixture-user", "from batocera.conf");
        assert_eq!(account.api_key.as_deref(), Some("k3y&/"));
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
