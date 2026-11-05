//! libretro thumbnails (thumbnails.libretro.com): boxes, screenshots, title screens and logos
//! named after No-Intro and Redump sets, so they match ROMs named that way. No account
//! needed. Nothing is searched: the URLs are guessed from the file name and the game's name,
//! and the engine moves on when one is not there.

use super::super::text::encode;
use super::super::{Credentials, Found, LookupError, MediaKind, Query, Source};
use crate::net::Http;
use crate::scraper::platforms;

const GIVES: [MediaKind; 4] = [
    MediaKind::Thumbnail,
    MediaKind::Image,
    MediaKind::Titleshot,
    MediaKind::Wheel,
];

pub struct Libretro;

/// libretro's rule for file names: these become `_`.
fn file_name(name: &str) -> String {
    name.chars()
        .map(|c| if "&*/:`<>?\\|\"".contains(c) { '_' } else { c })
        .collect()
}

impl Source for Libretro {
    fn id(&self) -> &str {
        "libretro"
    }
    fn name(&self) -> &str {
        "libretro thumbnails"
    }
    fn about(&self) -> &str {
        "Boxes, screenshots, title screens and logos for ROMs with No-Intro or Redump names. \
         Nothing to set up"
    }
    fn gives(&self) -> &[MediaKind] {
        &GIVES
    }
    fn has_metadata(&self) -> bool {
        false
    }
    fn covers(&self, query: &Query<'_>) -> bool {
        platforms::libretro_folder(query.system).is_some()
    }

    fn lookup(
        &self,
        _: &dyn Http,
        _: &Credentials,
        query: &Query<'_>,
    ) -> Result<Option<Found>, LookupError> {
        let Some(folder) = platforms::libretro_folder(query.system) else {
            return Ok(None);
        };
        let mut names = vec![query.stem.to_owned()];
        if !query.name.is_empty() && query.name != query.stem {
            names.push(query.name.to_owned());
        }
        let mut media = Vec::new();
        for (kind, dir) in [
            (MediaKind::Thumbnail, "Named_Boxarts"),
            (MediaKind::Image, "Named_Snaps"),
            (MediaKind::Titleshot, "Named_Titles"),
            (MediaKind::Wheel, "Named_Logos"),
        ] {
            if !query.wanted.contains(&kind) {
                continue;
            }
            for name in &names {
                media.push((
                    kind,
                    format!(
                        "https://thumbnails.libretro.com/{}/{dir}/{}.png",
                        encode(folder),
                        encode(&file_name(name))
                    ),
                ));
            }
        }
        Ok((!media.is_empty()).then_some(Found {
            media,
            ..Found::default()
        }))
    }
}
