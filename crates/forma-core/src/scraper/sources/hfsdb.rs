//! HFSDB (db.hfsplay.fr), a community database: boxes, screenshots, logos, videos, manuals and
//! details, searched by name on a system, or by MAME name for arcade. The team gives out API
//! access on its Discord.
//! Requests and answers follow EmulationStation's own HFSDB scraper.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::super::pick::base64;
use super::super::text::{best_match, date, encode, json_text, search_name};
use super::super::{
    Credential, Credentials, Found, LookupError, MediaKind, Metadata, Query, Source, platforms,
};
use super::{failed, json};
use crate::net::{Http, HttpRequest};

const API: &str = "https://db.hfsplay.fr/api/v1";
/// How often EmulationStation renews its token; HFSDB does not say how long one lasts.
const TOKEN_LIFE: Duration = Duration::from_secs(30 * 60);
/// Its one system for arcade, searched by file rather than by name.
const ARCADE: &str = "29794";

static NEEDS: [Credential; 2] = [
    Credential::new("username", "HFSDB user name", false, true),
    Credential::new("password", "HFSDB password", true, true),
];

const GIVES: [MediaKind; 10] = [
    MediaKind::Thumbnail,
    MediaKind::Boxback,
    MediaKind::Boxart,
    MediaKind::Image,
    MediaKind::Titleshot,
    MediaKind::Wheel,
    MediaKind::Marquee,
    MediaKind::Fanart,
    MediaKind::Video,
    MediaKind::Manual,
];

/// For each kind, HFSDB's media type and, where it has one, what kind of that type; best first.
const WANTED: [(MediaKind, &str, Option<&str>); 15] = [
    (MediaKind::Thumbnail, "cover2d", Some("front")),
    (MediaKind::Thumbnail, "cover2d", None),
    (MediaKind::Thumbnail, "artwork", Some("Flyer")),
    (MediaKind::Boxback, "cover2d", Some("back")),
    (MediaKind::Boxart, "cover3d", None),
    (MediaKind::Image, "screenshot", Some("in game")),
    (MediaKind::Titleshot, "screenshot", Some("title")),
    (MediaKind::Wheel, "logo", None),
    (MediaKind::Marquee, "wheel", None),
    (MediaKind::Marquee, "artwork", Some("Marquee")),
    (MediaKind::Fanart, "wallpaper", None),
    (MediaKind::Fanart, "artwork", None),
    (MediaKind::Video, "video", None),
    (MediaKind::Manual, "manual", None),
    (MediaKind::Image, "screenshot", None),
];

#[derive(Default)]
pub struct HfsDb {
    /// The token, for which user, and when it was issued.
    token: Mutex<Option<(String, String, Instant)>>,
}

impl HfsDb {
    fn token(&self, http: &dyn Http, user: &str, password: &str) -> Result<String, LookupError> {
        let mut held = self
            .token
            .lock()
            .map_err(|_| LookupError::Failed("poisoned".into()))?;
        if let Some((token, for_user, at)) = held.as_ref()
            && for_user == user
            && at.elapsed() < TOKEN_LIFE
        {
            return Ok(token.clone());
        }
        let form = format!("username={}&password={}", encode(user), encode(password));
        let basic = base64(format!("{user}:{password}").as_bytes());
        let request = HttpRequest::post(format!("{API}/auth/token"), form)
            .header("Authorization", format!("Basic {basic}"))
            .header("Content-Type", "application/x-www-form-urlencoded");
        let doc: Value = match http.send(&request) {
            Ok(body) => {
                serde_json::from_slice(&body).map_err(|e| LookupError::Failed(e.to_string()))?
            }
            Err(e) if matches!(e.status, Some(400 | 401 | 403)) => {
                return Err(LookupError::Credentials(
                    "HFSDB refused the user name or password".into(),
                ));
            }
            Err(e) => return Err(failed("HFSDB", &e)),
        };
        let token = doc["token"]
            .as_str()
            .ok_or_else(|| LookupError::Credentials("HFSDB gave no token".into()))?
            .to_owned();
        *held = Some((token.clone(), user.to_owned(), Instant::now()));
        Ok(token)
    }
}

impl Source for HfsDb {
    fn id(&self) -> &str {
        "hfsdb"
    }
    fn name(&self) -> &str {
        "HFSDB"
    }
    fn about(&self) -> &str {
        "Boxes, screenshots, logos, videos, manuals and details, by name. Needs API access: ask \
         the HFSDB team on their Discord (see db.hfsplay.fr/faq)"
    }
    fn needs(&self) -> &[Credential] {
        &NEEDS
    }
    fn gives(&self) -> &[MediaKind] {
        &GIVES
    }
    fn covers(&self, query: &Query<'_>) -> bool {
        !platforms::hfsdb(query.platforms).is_empty()
    }

    fn lookup(
        &self,
        http: &dyn Http,
        credentials: &Credentials,
        query: &Query<'_>,
    ) -> Result<Option<Found>, LookupError> {
        let systems = platforms::hfsdb(query.platforms);
        if systems.is_empty() {
            return Ok(None);
        }
        let user = credentials.get("username").map_or("", |k| k.trim());
        let password = credentials.get("password").map_or("", |k| k.trim());
        let token = self.token(http, user, password)?;
        let arcade = systems.contains(&ARCADE);
        let urls: Vec<String> = if arcade {
            vec![format!(
                "{API}/games?medias__description={}&system_id={ARCADE}",
                encode(&search_name(query.stem))
            )]
        } else {
            systems
                .iter()
                .map(|system| {
                    format!(
                        "{API}/games?search={}&system={system}&limit=25",
                        encode(query.name)
                    )
                })
                .collect()
        };
        for url in urls {
            let request = HttpRequest::get(url).header("Authorization", format!("Token {token}"));
            let Some(doc) = json(http, self.name(), &request)? else {
                continue;
            };
            let games = doc["results"].as_array().map_or(&[][..], Vec::as_slice);
            let game = if arcade {
                games.iter().find(|g| {
                    g["system"]["id"]
                        .as_u64()
                        .is_none_or(|id| !platforms::hfsdb_is_a_home_system(id))
                })
            } else {
                let names = games.iter().map(|g| {
                    g["name_en"]
                        .as_str()
                        .or_else(|| g["name_fr"].as_str())
                        .unwrap_or("")
                });
                best_match(query.name, names).and_then(|i| games.get(i))
            };
            if let Some(game) = game {
                return Ok(Some(parse(game)));
            }
        }
        Ok(None)
    }
}

/// What kind of its type a media is: in its `metadata` (`cover2dtype`: `front`) or description.
fn is_a(media: &Value, kind: &str, tag: &str) -> bool {
    let key = format!("{kind}type");
    media["metadata"].as_array().is_some_and(|all| {
        all.iter()
            .any(|m| m["name"].as_str() == Some(&key) && m["value"].as_str() == Some(tag))
    }) || media["description"].as_str() == Some(tag)
}

pub(super) fn parse(game: &Value) -> Found {
    let all = game["medias"].as_array().map_or(&[][..], Vec::as_slice);
    let mut media: Vec<(MediaKind, String)> = Vec::new();
    for (kind, of_type, tag) in WANTED {
        for item in all.iter().filter(|m| m["type"].as_str() == Some(of_type)) {
            let right = match tag {
                Some(tag) => is_a(item, of_type, tag),
                // A back is never a front, however it fell through.
                None => kind != MediaKind::Thumbnail || !is_a(item, of_type, "back"),
            };
            if !right {
                continue;
            }
            if let Some(url) = item["file"].as_str().filter(|u| u.starts_with("http"))
                && !media.iter().any(|(k, u)| *k == kind && u == url)
            {
                media.push((kind, url.to_owned()));
            }
        }
    }

    let details = game["metadata"].as_array().map_or(&[][..], Vec::as_slice);
    let detail = |name: &str| {
        details
            .iter()
            .find(|m| m["name"].as_str() == Some(name))
            .and_then(|m| json_text(m.get("value")))
    };
    let release_date = [
        "released_at_WORLD",
        "released_at_US",
        "released_at_PAL",
        "released_at_JPN",
    ]
    .iter()
    .find_map(|key| game[*key].as_str().and_then(date));
    Found {
        metadata: Metadata {
            name: json_text(game.get("name_en")),
            description: json_text(game.get("description_en")),
            developer: detail("developer"),
            publisher: detail("editor").or_else(|| detail("manufacturer")),
            genre: detail("genre"),
            players: detail("players").map(|p| {
                p.replace(" joueurs", "")
                    .replace(" joueur", "")
                    .replace("+ de ", "")
            }),
            release_date,
            rating: None,
            age_rating: None,
        },
        media,
    }
}
