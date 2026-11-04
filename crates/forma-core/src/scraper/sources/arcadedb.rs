//! ArcadeDB (adb.arcadeitalia.net): arcade games by MAME short name; in-game and title shots,
//! marquees, flyers, cabinets, videos and details. No account needed.

use serde_json::Value;

use super::super::text::{date, encode, json_text};
use super::super::{Credentials, Found, LookupError, MediaKind, Metadata, Query, Source};
use super::json;
use crate::net::{Http, HttpRequest};
use crate::scraper::platforms;

const GIVES: [MediaKind; 6] = [
    MediaKind::Image,
    MediaKind::Titleshot,
    MediaKind::Marquee,
    MediaKind::Thumbnail,
    MediaKind::Boxart,
    MediaKind::Video,
];

pub struct ArcadeDb;

impl Source for ArcadeDb {
    fn id(&self) -> &str {
        "arcadedb"
    }
    fn name(&self) -> &str {
        "ArcadeDB"
    }
    fn about(&self) -> &str {
        "Arcade games by their MAME name: shots, marquees, flyers (as the box), cabinets (as \
         the 3D box) and videos. Nothing to set up"
    }
    fn gives(&self) -> &[MediaKind] {
        &GIVES
    }
    fn covers(&self, query: &Query<'_>) -> bool {
        platforms::is_arcade(query.system, query.platforms)
    }

    fn lookup(
        &self,
        http: &dyn Http,
        _: &Credentials,
        query: &Query<'_>,
    ) -> Result<Option<Found>, LookupError> {
        let url = format!(
            "https://adb.arcadeitalia.net/service_scraper.php?ajax=query_mame&lang=en&use_parent=1&game_name={}",
            encode(query.stem)
        );
        let Some(doc) = json(http, self.name(), &HttpRequest::get(url))? else {
            return Ok(None);
        };
        Ok(doc["result"].get(0).map(parse))
    }
}

pub(super) fn parse(game: &Value) -> Found {
    let mut media = Vec::new();
    for (kind, keys) in [
        (MediaKind::Image, &["url_image_ingame"][..]),
        (MediaKind::Titleshot, &["url_image_title"]),
        (MediaKind::Marquee, &["url_image_marquee"]),
        (MediaKind::Thumbnail, &["url_image_flyer"]),
        (MediaKind::Boxart, &["url_image_cabinet"]),
        (
            MediaKind::Video,
            &["url_video_shortplay_hd", "url_video_shortplay"],
        ),
    ] {
        for key in keys {
            if let Some(url) = json_text(game.get(*key)) {
                media.push((kind, url));
            }
        }
    }
    let manufacturer = json_text(game.get("manufacturer"));
    let description = json_text(game.get("history")).map(|h| {
        // The history opens with "Arcade Video game published N years ago:" and ends with
        // credits and technical notes, after a "- " heading.
        let h = h.replace("\r\n", "\n");
        let body = h.split("\n- ").next().unwrap_or(&h).trim();
        match body.split_once('\n') {
            Some((first, rest)) if first.starts_with("Arcade Video game published") => {
                rest.trim().to_owned()
            }
            _ => body.to_owned(),
        }
    });
    Found {
        metadata: Metadata {
            name: json_text(game.get("title")),
            description,
            developer: manufacturer.clone(),
            publisher: manufacturer,
            genre: json_text(game.get("genre")),
            players: json_text(game.get("players")),
            release_date: json_text(game.get("year")).and_then(|y| date(&y)),
            rating: None,
        },
        media,
    }
}
