//! IGDB (igdb.com, Twitch's): searched by name and platform; covers, screenshots, artwork and
//! full metadata. Needs a Twitch application's client ID and secret (dev.twitch.tv).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::super::text::{age_ratings, best_match, date_from_unix, encode, json_number, json_text};
use super::super::{
    Credential, Credentials, Found, LookupError, MediaKind, Metadata, Query, Source,
};
use super::json;
use crate::net::{Http, HttpRequest};
use crate::scraper::platforms;

static NEEDS: [Credential; 2] = [
    Credential::new("clientId", "Twitch client ID", false, true),
    Credential::new("clientSecret", "Twitch client secret", true, true),
];

const GIVES: [MediaKind; 3] = [MediaKind::Thumbnail, MediaKind::Image, MediaKind::Fanart];

#[derive(Default)]
pub struct Igdb {
    /// The access token, for which client ID, and until when.
    token: Mutex<Option<(String, String, Instant)>>,
}

impl Igdb {
    fn token(&self, http: &dyn Http, id: &str, secret: &str) -> Result<String, LookupError> {
        let mut held = self
            .token
            .lock()
            .map_err(|_| LookupError::Failed("poisoned".into()))?;
        if let Some((token, for_id, until)) = held.as_ref()
            && for_id == id
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let url = format!(
            "https://id.twitch.tv/oauth2/token?client_id={}&client_secret={}&grant_type=client_credentials",
            encode(id),
            encode(secret)
        );
        let doc = match http.send(&HttpRequest::post(url, Vec::new())) {
            Ok(body) => serde_json::from_slice::<Value>(&body)
                .map_err(|e| LookupError::Failed(e.to_string()))?,
            Err(e) if matches!(e.status, Some(400 | 401 | 403)) => {
                return Err(LookupError::Credentials(
                    "Twitch refused the client ID or secret".into(),
                ));
            }
            Err(e) => return Err(LookupError::Failed(e.message)),
        };
        let token = doc["access_token"]
            .as_str()
            .ok_or_else(|| LookupError::Credentials("Twitch gave no token".into()))?
            .to_owned();
        let life = doc["expires_in"]
            .as_u64()
            .unwrap_or(3600)
            .saturating_sub(60);
        *held = Some((
            token.clone(),
            id.to_owned(),
            Instant::now() + Duration::from_secs(life),
        ));
        Ok(token)
    }
}

impl Source for Igdb {
    fn id(&self) -> &str {
        "igdb"
    }
    fn name(&self) -> &str {
        "IGDB"
    }
    fn about(&self) -> &str {
        "Covers, screenshots, artwork and full details, by name. Needs a Twitch app's client ID \
         and secret (dev.twitch.tv)"
    }
    fn needs(&self) -> &[Credential] {
        &NEEDS
    }
    fn gives(&self) -> &[MediaKind] {
        &GIVES
    }

    fn lookup(
        &self,
        http: &dyn Http,
        credentials: &Credentials,
        query: &Query<'_>,
    ) -> Result<Option<Found>, LookupError> {
        let id = credentials.get("clientId").map_or("", |k| k.trim());
        let secret = credentials.get("clientSecret").map_or("", |k| k.trim());
        let token = self.token(http, id, secret)?;
        let name = query.name.replace(['"', '\\'], " ");
        let mut body = format!(
            "search \"{name}\"; fields name,summary,first_release_date,genres.name,\
             involved_companies.company.name,involved_companies.developer,\
             involved_companies.publisher,total_rating,cover.image_id,screenshots.image_id,\
             artworks.image_id,game_modes.name,multiplayer_modes.offlinemax,\
             age_ratings.organization.name,age_ratings.rating_category.rating; limit 10;"
        );
        if let Some(platform) = platforms::ids(query.platforms).igdb {
            body.push_str(&format!(" where platforms = ({platform});"));
        }
        let request = HttpRequest::post("https://api.igdb.com/v4/games", body)
            .header("Client-ID", id)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json");
        let doc = match json(http, self.name(), &request) {
            Err(LookupError::Credentials(e)) => {
                // The token may have been revoked: get a new one next time.
                *self
                    .token
                    .lock()
                    .map_err(|_| LookupError::Failed("poisoned".into()))? = None;
                return Err(LookupError::Credentials(e));
            }
            other => other?,
        };
        let games = doc
            .as_ref()
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let Some(i) = best_match(
            query.name,
            games.iter().map(|g| g["name"].as_str().unwrap_or("")),
        ) else {
            return Ok(None);
        };
        Ok(Some(parse(&games[i])))
    }
}

pub(super) fn parse(game: &Value) -> Found {
    let companies = game["involved_companies"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let company = |role: &str| {
        let names: Vec<&str> = companies
            .iter()
            .filter(|c| c[role].as_bool() == Some(true))
            .filter_map(|c| c["company"]["name"].as_str())
            .collect();
        (!names.is_empty()).then(|| names.join(" / "))
    };
    let genres: Vec<&str> = game["genres"]
        .as_array()
        .map(|g| g.iter().filter_map(|g| g["name"].as_str()).collect())
        .unwrap_or_default();
    let players = game["multiplayer_modes"]
        .as_array()
        .and_then(|m| m.iter().filter_map(|m| m["offlinemax"].as_u64()).max())
        .filter(|n| *n > 1)
        .map(|n| format!("1-{n}"))
        .or_else(|| {
            let modes = game["game_modes"].as_array()?;
            modes
                .iter()
                .all(|m| m["name"].as_str() == Some("Single player"))
                .then(|| "1".to_owned())
        });
    let image = |id: &Value, size: &str| {
        id.as_str()
            .map(|id| format!("https://images.igdb.com/igdb/image/upload/{size}/{id}.jpg"))
    };
    let mut media = Vec::new();
    if let Some(url) = image(&game["cover"]["image_id"], "t_cover_big_2x") {
        media.push((MediaKind::Thumbnail, url));
    }
    for (list, kind, size) in [
        ("screenshots", MediaKind::Image, "t_screenshot_huge"),
        ("artworks", MediaKind::Fanart, "t_1080p"),
    ] {
        for item in game[list].as_array().map(Vec::as_slice).unwrap_or_default() {
            if let Some(url) = image(&item["image_id"], size) {
                media.push((kind, url));
            }
        }
    }
    Found {
        metadata: Metadata {
            name: json_text(game.get("name")),
            description: json_text(game.get("summary")),
            developer: company("developer"),
            publisher: company("publisher"),
            genre: (!genres.is_empty()).then(|| genres.join(" / ")),
            players,
            release_date: game["first_release_date"].as_i64().map(date_from_unix),
            rating: json_number(game.get("total_rating")).map(|r| r / 100.0),
            age_rating: age_ratings(
                game["age_ratings"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|r| {
                        let board = r["organization"]["name"].as_str()?;
                        Some((board, igdb_rating(r["rating_category"]["rating"].as_str()?)))
                    }),
            ),
        },
        media,
        notes: Default::default(),
    }
}

/// IGDB's rating names as boards print them: `Eighteen` is `18`, `E10` is `E10+`, `CERO_A`
/// is `A`, `USK_12` is `12`.
fn igdb_rating(rating: &str) -> &str {
    let rating = rating.rsplit('_').next().unwrap_or(rating);
    match rating {
        "Three" => "3",
        "Seven" => "7",
        "Twelve" => "12",
        "Sixteen" => "16",
        "Eighteen" => "18",
        "E10" => "E10+",
        other => other,
    }
}
