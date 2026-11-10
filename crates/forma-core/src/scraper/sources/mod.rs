//! The sources built into Forma. Add-ons add more through the same [`Source`] trait.

mod arcadedb;
mod hfsdb;
mod igdb;
mod launchbox;
mod libretro;
mod mobygames;
pub(super) mod screenscraper;
mod steamgriddb;
mod thegamesdb;

use super::{LookupError, Source};
use crate::net::{HttpError, HttpRequest};

/// Every built-in source, in the order a new user gets them.
pub fn builtin() -> Vec<Box<dyn Source>> {
    vec![
        Box::new(screenscraper::ScreenScraper),
        Box::new(thegamesdb::TheGamesDb::default()),
        Box::new(igdb::Igdb::default()),
        Box::new(mobygames::MobyGames),
        Box::new(hfsdb::HfsDb::default()),
        Box::new(steamgriddb::SteamGridDb),
        Box::new(arcadedb::ArcadeDb),
        Box::new(launchbox::LaunchBox::default()),
        Box::new(libretro::Libretro),
    ]
}

/// What a failed request means for most JSON APIs: 401/403 the credentials, 429 the quota,
/// 404 not known (`Ok(None)` from the caller).
fn failed(name: &str, e: &HttpError) -> LookupError {
    match e.status {
        Some(401 | 403) => LookupError::Credentials(format!("{name} refused the credentials")),
        Some(429) => LookupError::Quota(format!("{name}: too many requests for now")),
        _ => LookupError::Failed(e.message.clone()),
    }
}

/// Sends `request` and parses JSON; `Ok(None)` on 404.
fn json(
    http: &dyn crate::net::Http,
    name: &str,
    request: &HttpRequest,
) -> Result<Option<serde_json::Value>, LookupError> {
    match http.send(request) {
        Ok(body) => serde_json::from_slice(&body)
            .map(Some)
            .map_err(|e| LookupError::Failed(format!("{name} sent something unreadable: {e}"))),
        Err(e) if e.status == Some(404) => Ok(None),
        Err(e) => Err(failed(name, &e)),
    }
}

#[cfg(test)]
mod tests;
