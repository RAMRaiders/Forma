//! MobyGames (mobygames.com): searched by title; covers, screenshots, descriptions and
//! genres. Needs an API key from the user's MobyGames account.

use serde_json::Value;

use super::super::text::{best_match, date, encode, json_number, json_text, strip_html};
use super::super::{
    Credential, Credentials, Found, LookupError, MediaKind, Metadata, Query, Source,
};
use super::json;
use crate::net::{Http, HttpRequest};

const NEEDS: [Credential; 1] = [Credential {
    key: "apiKey",
    label: "API key",
    secret: true,
    required: true,
}];

const GIVES: [MediaKind; 2] = [MediaKind::Thumbnail, MediaKind::Image];

pub struct MobyGames;

impl Source for MobyGames {
    fn id(&self) -> &str {
        "mobygames"
    }
    fn name(&self) -> &str {
        "MobyGames"
    }
    fn about(&self) -> &str {
        "Covers, screenshots, descriptions and genres, by title. Needs an API key (your \
         MobyGames account)"
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
        let key = credentials.get("apiKey").map_or("", |k| k.trim());
        let url = format!(
            "https://api.mobygames.com/v1/games?api_key={}&title={}&format=normal&limit=10",
            encode(key),
            encode(query.name)
        );
        let Some(doc) = json(http, self.name(), &HttpRequest::get(url))? else {
            return Ok(None);
        };
        let games = doc["games"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let Some(i) = best_match(
            query.name,
            games.iter().map(|g| g["title"].as_str().unwrap_or("")),
        ) else {
            return Ok(None);
        };
        Ok(Some(parse(&games[i])))
    }
}

pub(super) fn parse(game: &Value) -> Found {
    let genres: Vec<&str> = game["genres"]
        .as_array()
        .map(|g| {
            g.iter()
                .filter(|g| g["genre_category"].as_str() == Some("Basic Genres"))
                .filter_map(|g| g["genre_name"].as_str())
                .collect()
        })
        .unwrap_or_default();
    let release = game["platforms"]
        .as_array()
        .and_then(|p| {
            p.iter()
                .filter_map(|p| p["first_release_date"].as_str())
                .min()
        })
        .and_then(date);
    let mut media = Vec::new();
    if let Some(url) = game["sample_cover"]["image"].as_str() {
        media.push((MediaKind::Thumbnail, url.to_owned()));
    }
    for shot in game["sample_screenshots"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        if let Some(url) = shot["image"].as_str() {
            media.push((MediaKind::Image, url.to_owned()));
        }
    }
    Found {
        metadata: Metadata {
            name: json_text(game.get("title")),
            description: game["description"]
                .as_str()
                .map(strip_html)
                .filter(|d| !d.is_empty()),
            genre: (!genres.is_empty()).then(|| genres.join(" / ")),
            release_date: release,
            rating: json_number(game.get("moby_score")).map(|s| s / 10.0),
            ..Metadata::default()
        },
        media,
    }
}
