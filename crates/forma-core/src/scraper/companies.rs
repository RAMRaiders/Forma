//! Publishers' and developers' logos, from ScreenScraper with the user's own login. They stay
//! on this machine, one small PNG per company, for themes to show (`/company/<slug>`).

use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageFormat, RgbaImage};

use super::engine::{HASH_LIMIT, hash};
use super::sources::screenscraper;
use super::{Credentials, LookupError, Query};
use crate::library::Game;
use crate::net::Http;
use crate::paths::Paths;
use crate::systems::System;

/// Twice the height a theme is likely to show one at.
const HEIGHT: u32 = 96;
const DOWNLOAD_LIMIT: u64 = 8 << 20;

/// A company's name as its file is called: letters and digits, the rest dashes. Empty for a
/// name with neither. The SDK's `companySlug` does the same.
pub fn slug(name: &str) -> String {
    name.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Where a company's logo goes.
pub fn file(paths: &Paths, slug: &str) -> PathBuf {
    paths
        .forma_dir()
        .join("company-logos")
        .join(format!("{slug}.png"))
}

/// The logo saved for `slug`, if it is one a request may name and there is one.
pub fn logo(paths: &Paths, slug: &str) -> Option<PathBuf> {
    let valid = !slug.is_empty() && slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    valid.then(|| file(paths, slug)).filter(|f| f.is_file())
}

/// The logos `game`'s publisher and developer still lack.
pub fn lacking(paths: &Paths, game: &Game) -> BTreeSet<String> {
    [game.publisher.as_deref(), game.developer.as_deref()]
        .into_iter()
        .flatten()
        .map(slug)
        .filter(|s| !s.is_empty() && logo(paths, s).is_none())
        .collect()
}

/// Asks ScreenScraper who made `game` and saves the logos still missing; returns how many.
pub fn fetch(
    http: &dyn Http,
    paths: &Paths,
    credentials: &Credentials,
    game: &Game,
    system: &System,
) -> Result<usize, LookupError> {
    let rom = Path::new(&game.path);
    let name_of = |n: Option<&std::ffi::OsStr>| {
        n.map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let file_name = name_of(rom.file_name());
    let stem = name_of(rom.file_stem());
    let size = std::fs::metadata(rom).map_or(0, |m| m.len());
    let hashes = (size > 0 && size <= HASH_LIMIT)
        .then(|| hash(rom))
        .flatten();
    let query = Query {
        system: &system.name,
        platforms: &system.platforms,
        file_name: &file_name,
        stem: &stem,
        name: &game.name,
        size,
        hashes: hashes.as_ref(),
        wanted: &[],
        wants_metadata: true,
    };
    let mut saved = 0;
    for art in screenscraper::companies(http, credentials, &query)? {
        // Filed under the name the library knows it by, and ScreenScraper's where that differs.
        let ours = if art.publisher {
            &game.publisher
        } else {
            &game.developer
        };
        let slugs: BTreeSet<String> = ours
            .iter()
            .map(|n| slug(n))
            .chain([slug(&art.name)])
            .filter(|s| !s.is_empty())
            .collect();
        if slugs.iter().all(|s| logo(paths, s).is_some()) {
            continue;
        }
        let Some(png) = art
            .urls
            .iter()
            .find_map(|url| prepared(&http.download(url, DOWNLOAD_LIMIT).ok()?))
        else {
            continue;
        };
        for s in &slugs {
            if write(&file(paths, s), &png).is_err() {
                return Err(LookupError::Failed(format!("could not save the {s} logo")));
            }
        }
        saved += 1;
    }
    Ok(saved)
}

fn write(path: &Path, png: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("forma.tmp");
    std::fs::write(&temp, png)?;
    std::fs::rename(&temp, path)
}

/// `bytes` as a small PNG, if they are a picture: ScreenScraper answers `NOMEDIA` otherwise. A
/// cut-out logo too dark to see on a dark screen is made white.
fn prepared(bytes: &[u8]) -> Option<Vec<u8>> {
    let picture = image::load_from_memory(bytes).ok()?;
    let small = if picture.height() > HEIGHT {
        picture.resize(u32::MAX, HEIGHT, image::imageops::FilterType::Lanczos3)
    } else {
        picture
    };
    let mut pixels = small.to_rgba8();
    whiten_if_dark(&mut pixels);
    let mut png = Vec::new();
    DynamicImage::ImageRgba8(pixels)
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .ok()?;
    Some(png)
}

fn whiten_if_dark(pixels: &mut RgbaImage) {
    let linear = |c: u8| {
        let v = f32::from(c) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let (mut sum, mut opaque) = (0.0, 0u32);
    for p in pixels.pixels().filter(|p| p[3] >= 128) {
        sum += 0.2126 * linear(p[0]) + 0.7152 * linear(p[1]) + 0.0722 * linear(p[2]);
        opaque += 1;
    }
    let cut_out = f64::from(opaque) < f64::from(pixels.width() * pixels.height()) * 0.85;
    if opaque > 0 && cut_out && sum / (opaque as f32) < 0.06 {
        for p in pixels.pixels_mut() {
            p[0] = 255;
            p[1] = 255;
            p[2] = 255;
        }
    }
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    #[test]
    fn a_name_becomes_a_safe_file_name() {
        assert_eq!(slug("Namco Bandai Games"), "namco-bandai-games");
        assert_eq!(slug("I'Max"), "i-max");
        assert_eq!(slug("A.I Co"), "a-i-co");
        assert_eq!(slug("ナムコ"), "");
    }

    #[test]
    fn a_request_cannot_name_anything_but_a_logo() {
        let paths = Paths {
            userdata: std::env::temp_dir().join("forma-no-such-userdata"),
            systems_cfg: PathBuf::new(),
        };
        assert_eq!(logo(&paths, "../../etc/passwd"), None);
        assert_eq!(logo(&paths, ""), None);
    }

    fn png_of(colour: [u8; 4], covered: u32) -> Vec<u8> {
        let mut image = RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 0]));
        for (i, p) in image.pixels_mut().enumerate() {
            if (i as u32) < covered {
                *p = Rgba(colour);
            }
        }
        let mut png = Vec::new();
        DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        png
    }

    fn first_pixel(png: &[u8]) -> [u8; 4] {
        image::load_from_memory(png)
            .unwrap()
            .to_rgba8()
            .get_pixel(0, 0)
            .0
    }

    #[test]
    fn a_black_cut_out_turns_white_but_a_colour_or_a_box_stays() {
        let black = prepared(&png_of([0, 0, 0, 255], 30)).unwrap();
        assert_eq!(first_pixel(&black), [255, 255, 255, 255]);
        let red = prepared(&png_of([220, 20, 20, 255], 30)).unwrap();
        assert_eq!(first_pixel(&red), [220, 20, 20, 255]);
        let boxed = prepared(&png_of([0, 0, 0, 255], 100)).unwrap();
        assert_eq!(first_pixel(&boxed), [0, 0, 0, 255]);
    }

    #[test]
    fn nomedia_is_no_picture() {
        assert_eq!(prepared(b"NOMEDIA"), None);
    }
}
