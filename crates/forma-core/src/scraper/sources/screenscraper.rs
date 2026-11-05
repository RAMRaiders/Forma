//! ScreenScraper (screenscraper.fr): matches by hash, then by file name and size, and has
//! every kind of media. Its API needs a developer ID and password (which the user requests on
//! the site) and works better with the user's own login, which raises their quota.

use serde_json::Value;

use super::super::text::{date, json_number, json_text};
use super::super::{
    Credential, Credentials, Found, LookupError, MediaKind, Metadata, Query, Source,
};
use crate::net::{Http, HttpRequest};
use crate::scraper::platforms;
use crate::scraper::text::encode;

pub struct ScreenScraper;

static NEEDS: [Credential; 4] = [
    Credential::new("devId", "Developer ID", false, true),
    Credential::new("devPassword", "Developer password", true, true),
    Credential::new("user", "Username", false, false),
    Credential::new("password", "Password", true, false),
];

/// Its media types for each kind, best first.
const TYPES: [(MediaKind, &[&str]); 14] = [
    (MediaKind::Image, &["ss"]),
    (MediaKind::Titleshot, &["sstitle"]),
    (MediaKind::Thumbnail, &["box-2D"]),
    (MediaKind::Boxback, &["box-2D-back"]),
    (MediaKind::Boxart, &["box-3D"]),
    (MediaKind::Marquee, &["screenmarqueesmall", "screenmarquee"]),
    (
        MediaKind::Wheel,
        &["wheel-hd", "wheel", "wheel-carbon", "wheel-steel"],
    ),
    (MediaKind::Fanart, &["fanart"]),
    (MediaKind::Cartridge, &["support-2D", "support-texture"]),
    (MediaKind::Map, &["maps"]),
    (MediaKind::Bezel, &["bezel-16-9"]),
    (MediaKind::Mix, &["mixrbv2", "mixrbv1"]),
    (MediaKind::Video, &["video-normalized", "video"]),
    (MediaKind::Manual, &["manuel"]),
];

const REGIONS: [&str; 8] = ["wor", "us", "eu", "ss", "uk", "fr", "de", "jp"];

impl Source for ScreenScraper {
    fn id(&self) -> &str {
        "screenscraper"
    }
    fn name(&self) -> &str {
        "ScreenScraper"
    }
    fn about(&self) -> &str {
        "Every kind of media, matched by the ROM's hash. Needs a developer ID and password \
         (ask for them on screenscraper.fr); your own login raises how many games a day"
    }
    fn needs(&self) -> &[Credential] {
        &NEEDS
    }
    fn gives(&self) -> &[MediaKind] {
        &MediaKind::ALL
    }

    fn lookup(
        &self,
        http: &dyn Http,
        credentials: &Credentials,
        query: &Query<'_>,
    ) -> Result<Option<Found>, LookupError> {
        let get = |k: &str| credentials.get(k).map_or("", |v| v.trim());
        let mut url = format!(
            "https://api.screenscraper.fr/api2/jeuInfos.php?devid={}&devpassword={}\
             &softname=Forma&output=json&romtype=rom&romnom={}",
            encode(get("devId")),
            encode(get("devPassword")),
            encode(query.file_name)
        );
        if !get("user").is_empty() {
            url.push_str(&format!(
                "&ssid={}&sspassword={}",
                encode(get("user")),
                encode(get("password"))
            ));
        }
        if let Some(id) = platforms::ids(query.platforms).screenscraper {
            url.push_str(&format!("&systemeid={id}"));
        }
        if query.size > 0 {
            url.push_str(&format!("&romtaille={}", query.size));
        }
        if let Some(h) = query.hashes {
            url.push_str(&format!("&crc={}&md5={}&sha1={}", h.crc32, h.md5, h.sha1));
        }
        let body = match http.send(&HttpRequest::get(url)) {
            Ok(body) => body,
            Err(e) => {
                return match e.status {
                    Some(404) => Ok(None),
                    Some(401 | 403) => Err(LookupError::Credentials(
                        "ScreenScraper refused the developer ID or password".into(),
                    )),
                    Some(423) => Err(LookupError::Quota(
                        "ScreenScraper's API is closed for now".into(),
                    )),
                    Some(426) => Err(LookupError::Credentials(
                        "ScreenScraper no longer accepts this software name".into(),
                    )),
                    Some(429..=431) => Err(LookupError::Quota(
                        "ScreenScraper: today's quota is used up".into(),
                    )),
                    _ => Err(LookupError::Failed(e.message)),
                };
            }
        };
        // Errors also come back as 200 with text, e.g. "Erreur de login".
        let Ok(doc) = serde_json::from_slice::<Value>(&body) else {
            let text = String::from_utf8_lossy(&body);
            if text.to_lowercase().contains("login") {
                return Err(LookupError::Credentials(
                    "ScreenScraper refused the username or password".into(),
                ));
            }
            return Ok(None);
        };
        Ok(parse(&doc["response"]["jeu"]))
    }
}

/// The text of the entry for the first region (or language) of `order` there is.
fn by_region(list: &Value, key: &str, order: &[&str]) -> Option<String> {
    let items = list.as_array()?;
    order
        .iter()
        .find_map(|r| items.iter().find(|i| i[key].as_str() == Some(r)))
        .or_else(|| items.first())
        .and_then(|i| json_text(i.get("text")))
}

pub(super) fn parse(game: &Value) -> Option<Found> {
    if !game.is_object() {
        return None;
    }
    let genre = game["genres"].as_array().and_then(|genres| {
        let principal = genres
            .iter()
            .find(|g| g["principale"].as_str() == Some("1"))
            .or_else(|| genres.first())?;
        by_region(&principal["noms"], "langue", &["en", "fr"])
    });
    let metadata = Metadata {
        name: by_region(&game["noms"], "region", &REGIONS),
        description: by_region(&game["synopsis"], "langue", &["en", "fr", "de", "es"]),
        developer: json_text(game["developpeur"].get("text")),
        publisher: json_text(game["editeur"].get("text")),
        genre,
        players: json_text(game["joueurs"].get("text")),
        release_date: by_region(&game["dates"], "region", &REGIONS).and_then(|d| date(&d)),
        rating: json_number(game["note"].get("text")).map(|n| n / 20.0),
    };
    let medias = game["medias"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut media = Vec::new();
    for (kind, types) in TYPES {
        for t in types {
            let mut of_type: Vec<&Value> = medias
                .iter()
                .filter(|m| m["type"].as_str() == Some(t))
                .collect();
            of_type.sort_by_key(|m| {
                REGIONS
                    .iter()
                    .position(|r| m["region"].as_str() == Some(r))
                    .unwrap_or(REGIONS.len())
            });
            for m in of_type {
                if let Some(url) = m["url"].as_str() {
                    media.push((kind, url.to_owned()));
                }
            }
        }
    }
    Some(Found { metadata, media })
}
