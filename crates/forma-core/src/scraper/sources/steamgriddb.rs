//! SteamGridDB (steamgriddb.com): community art by name; covers (grids), heroes (as fan art)
//! and logos. Media only. Needs an API key from the user's SteamGridDB preferences.

use super::super::text::{encode, json_text};
use super::super::{Credential, Credentials, Found, LookupError, MediaKind, Query, Source};
use super::json;
use crate::net::{Http, HttpRequest};

const API: &str = "https://www.steamgriddb.com/api/v2";

const NEEDS: [Credential; 1] = [Credential {
    key: "apiKey",
    label: "API key",
    secret: true,
    required: true,
}];

const GIVES: [MediaKind; 3] = [MediaKind::Thumbnail, MediaKind::Fanart, MediaKind::Wheel];

pub struct SteamGridDb;

impl Source for SteamGridDb {
    fn id(&self) -> &str {
        "steamgriddb"
    }
    fn name(&self) -> &str {
        "SteamGridDB"
    }
    fn about(&self) -> &str {
        "Community covers, heroes and logos, by name. Needs an API key (steamgriddb.com, \
         Preferences → API)"
    }
    fn needs(&self) -> &[Credential] {
        &NEEDS
    }
    fn gives(&self) -> &[MediaKind] {
        &GIVES
    }
    fn has_metadata(&self) -> bool {
        false
    }

    fn lookup(
        &self,
        http: &dyn Http,
        credentials: &Credentials,
        query: &Query<'_>,
    ) -> Result<Option<Found>, LookupError> {
        let key = credentials.get("apiKey").map_or("", |k| k.trim());
        let get = |path: String| {
            json(
                http,
                "SteamGridDB",
                &HttpRequest::get(format!("{API}{path}"))
                    .header("Authorization", format!("Bearer {key}")),
            )
        };
        let Some(search) = get(format!("/search/autocomplete/{}", encode(query.name)))? else {
            return Ok(None);
        };
        let Some(id) = json_text(search["data"].get(0).and_then(|g| g.get("id"))) else {
            return Ok(None);
        };
        let mut found = Found::default();
        for (kind, path) in [
            (
                MediaKind::Thumbnail,
                format!("/grids/game/{id}?dimensions=600x900,342x482,660x930"),
            ),
            (MediaKind::Fanart, format!("/heroes/game/{id}")),
            (MediaKind::Wheel, format!("/logos/game/{id}")),
        ] {
            if !query.wanted.contains(&kind) {
                continue;
            }
            if let Some(doc) = get(path)? {
                for item in doc["data"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .take(3)
                {
                    if let Some(url) = item["url"].as_str() {
                        found.media.push((kind, url.to_owned()));
                    }
                }
            }
        }
        Ok(Some(found))
    }
}
