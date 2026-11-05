//! TheGamesDB (thegamesdb.net): searched by name and platform; boxes front and back, fan art,
//! screenshots, logos, banners and title screens. Needs an API key from its forum.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;

use super::super::text::{best_match, date, encode, json_text};
use super::super::{
    Credential, Credentials, Found, LookupError, MediaKind, Metadata, Query, Source,
};
use super::json;
use crate::net::{Http, HttpRequest};
use crate::scraper::platforms;

const API: &str = "https://api.thegamesdb.net";

static NEEDS: [Credential; 1] = [Credential::new("apiKey", "API key", true, true)];

const GIVES: [MediaKind; 7] = [
    MediaKind::Thumbnail,
    MediaKind::Boxback,
    MediaKind::Image,
    MediaKind::Fanart,
    MediaKind::Wheel,
    MediaKind::Marquee,
    MediaKind::Titleshot,
];

/// Genres, developers and publishers come back as IDs; their names are fetched once a run.
#[derive(Default)]
pub struct TheGamesDb {
    names: Mutex<HashMap<&'static str, HashMap<String, String>>>,
}

impl TheGamesDb {
    fn names_of(
        &self,
        http: &dyn Http,
        key: &str,
        list: &'static str,
        ids: &Value,
    ) -> Option<String> {
        let ids: Vec<String> = ids
            .as_array()?
            .iter()
            .filter_map(|v| json_text(Some(v)))
            .collect();
        if ids.is_empty() {
            return None;
        }
        let mut names = self.names.lock().ok()?;
        if !names.contains_key(list) {
            let path = match list {
                "genres" => "Genres",
                "developers" => "Developers",
                _ => "Publishers",
            };
            let url = format!("{API}/v1/{path}?apikey={}", encode(key));
            let doc = json(http, "TheGamesDB", &HttpRequest::get(url))
                .ok()
                .flatten()?;
            let table = doc["data"][list]
                .as_object()?
                .iter()
                .filter_map(|(id, v)| Some((id.clone(), v["name"].as_str()?.to_owned())))
                .collect();
            names.insert(list, table);
        }
        let table = &names[list];
        let found: Vec<&str> = ids
            .iter()
            .filter_map(|id| table.get(id).map(String::as_str))
            .collect();
        (!found.is_empty()).then(|| found.join(" / "))
    }
}

impl Source for TheGamesDb {
    fn id(&self) -> &str {
        "thegamesdb"
    }
    fn name(&self) -> &str {
        "TheGamesDB"
    }
    fn about(&self) -> &str {
        "Boxes, fan art, logos and screenshots, by name. Needs an API key (thegamesdb.net forum)"
    }
    fn needs(&self) -> &[Credential] {
        &NEEDS
    }
    fn gives(&self) -> &[MediaKind] {
        &GIVES
    }
    fn covers(&self, query: &Query<'_>) -> bool {
        platforms::ids(query.platforms).thegamesdb.is_some()
    }

    fn lookup(
        &self,
        http: &dyn Http,
        credentials: &Credentials,
        query: &Query<'_>,
    ) -> Result<Option<Found>, LookupError> {
        let key = credentials.get("apiKey").map_or("", |k| k.trim());
        let Some(platform) = platforms::ids(query.platforms).thegamesdb else {
            return Ok(None);
        };
        let url = format!(
            "{API}/v1.1/Games/ByGameName?apikey={}&name={}&filter%5Bplatform%5D={platform}\
             &fields=players%2Cpublishers%2Cgenres%2Coverview%2Crating",
            encode(key),
            encode(query.name)
        );
        let Some(doc) = json(http, self.name(), &HttpRequest::get(url))? else {
            return Ok(None);
        };
        let games = doc["data"]["games"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let Some(i) = best_match(
            query.name,
            games.iter().map(|g| g["game_title"].as_str().unwrap_or("")),
        ) else {
            return Ok(None);
        };
        let game = &games[i];
        let Some(id) = json_text(game.get("id")) else {
            return Ok(None);
        };

        let mut found = Found::default();
        if query.wants_metadata {
            found.metadata = Metadata {
                name: json_text(game.get("game_title")),
                description: json_text(game.get("overview")),
                developer: self.names_of(http, key, "developers", &game["developers"]),
                publisher: self.names_of(http, key, "publishers", &game["publishers"]),
                genre: self.names_of(http, key, "genres", &game["genres"]),
                players: json_text(game.get("players")),
                release_date: json_text(game.get("release_date")).and_then(|d| date(&d)),
                rating: None,
            };
        }
        if GIVES.iter().any(|k| query.wanted.contains(k)) {
            let url = format!(
                "{API}/v1/Games/Images?apikey={}&games_id={id}\
                 &filter%5Btype%5D=boxart%2Cfanart%2Cbanner%2Cscreenshot%2Cclearlogo%2Ctitlescreen",
                encode(key)
            );
            if let Some(doc) = json(http, self.name(), &HttpRequest::get(url))? {
                found.media = images(&doc, &id);
            }
        }
        Ok(Some(found))
    }
}

pub(super) fn images(doc: &Value, id: &str) -> Vec<(MediaKind, String)> {
    let base = doc["data"]["base_url"]["original"]
        .as_str()
        .unwrap_or("https://cdn.thegamesdb.net/images/original/");
    let list = doc["data"]["images"][id]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut media = Vec::new();
    for (kind, kind_type, side) in [
        (MediaKind::Thumbnail, "boxart", Some("front")),
        (MediaKind::Boxback, "boxart", Some("back")),
        (MediaKind::Image, "screenshot", None),
        (MediaKind::Fanart, "fanart", None),
        (MediaKind::Wheel, "clearlogo", None),
        (MediaKind::Marquee, "banner", None),
        (MediaKind::Titleshot, "titlescreen", None),
    ] {
        for image in list {
            if image["type"].as_str() == Some(kind_type)
                && (side.is_none() || image["side"].as_str() == side)
                && let Some(file) = image["filename"].as_str()
            {
                media.push((kind, format!("{base}{file}")));
            }
        }
    }
    media
}
