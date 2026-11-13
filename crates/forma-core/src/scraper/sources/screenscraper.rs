//! ScreenScraper (screenscraper.fr): matches by hash, then by file name and size, and has
//! every kind of media. Its API needs a developer ID and password and works better with the
//! user's own login, which raises their quota. Release builds carry Forma's developer account
//! (see `docs/scraper.md`); other builds ask the user for one.

use serde_json::Value;

use super::super::text::{age_ratings, date, json_number, json_text};
use super::super::{
    Credential, Credentials, Found, LookupError, MediaKind, Metadata, Query, Source,
};
use crate::net::{Http, HttpRequest};
use crate::scraper::platforms;
use crate::scraper::text::encode;
use crate::scraper::{MediaNote, Notes};

pub struct ScreenScraper;

/// Forma's developer account, from the environment of the build. CI's secrets set these; an
/// unset secret arrives as an empty value, which counts as none.
const BUILT_IN_ID: &str = match option_env!("FORMA_SCREENSCRAPER_DEV_ID") {
    Some(id) => id,
    None => "",
};
const BUILT_IN_PASSWORD: &str = match option_env!("FORMA_SCREENSCRAPER_DEV_PASSWORD") {
    Some(password) => password,
    None => "",
};
pub(super) const HAS_BUILT_IN: bool = !BUILT_IN_ID.is_empty() && !BUILT_IN_PASSWORD.is_empty();

/// Masked so the account is not readable with `strings`. This only slows someone down: anyone
/// who takes the program apart can recover it.
static MASKED_ID: [u8; BUILT_IN_ID.len()] = mask(BUILT_IN_ID);
static MASKED_PASSWORD: [u8; BUILT_IN_PASSWORD.len()] = mask(BUILT_IN_PASSWORD);

const fn mask_byte(i: usize) -> u8 {
    0x5a_u8.wrapping_add((i as u8).wrapping_mul(37))
}

const fn mask<const N: usize>(text: &str) -> [u8; N] {
    let bytes = text.as_bytes();
    let mut masked = [0; N];
    let mut i = 0;
    while i < N {
        masked[i] = bytes[i] ^ mask_byte(i);
        i += 1;
    }
    masked
}

fn unmask(masked: &[u8]) -> String {
    let bytes = masked.iter().enumerate().map(|(i, b)| b ^ mask_byte(i));
    String::from_utf8_lossy(&bytes.collect::<Vec<_>>()).into_owned()
}

/// The developer account to use: the one the user entered, whole, or else the built-in one.
pub(super) fn developer(
    entered_id: &str,
    entered_password: &str,
    built_in: Option<(String, String)>,
) -> (String, String) {
    match built_in {
        Some(account) if entered_id.is_empty() || entered_password.is_empty() => account,
        _ => (entered_id.to_owned(), entered_password.to_owned()),
    }
}

static NEEDS: [Credential; 4] = [
    Credential::new("devId", "Developer ID", false, !HAS_BUILT_IN),
    Credential::new("devPassword", "Developer password", true, !HAS_BUILT_IN),
    Credential::new("user", "Username", false, false),
    Credential::new("password", "Password", true, false),
];

/// Its media types for each kind, best first.
const TYPES: [(MediaKind, &[&str]); 18] = [
    (MediaKind::Image, &["ss"]),
    (MediaKind::Titleshot, &["sstitle"]),
    (MediaKind::Thumbnail, &["box-2D"]),
    (MediaKind::Boxback, &["box-2D-back"]),
    (MediaKind::Boxside, &["box-2D-side"]),
    (MediaKind::Boxfull, &["box-texture"]),
    (MediaKind::Boxart, &["box-3D"]),
    (MediaKind::Banner, &["steamgrid"]),
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
    (MediaKind::Flyer, &["flyer"]),
];

/// Every kind it has a media type for (no cabinets).
const GIVES: [MediaKind; 18] = {
    let mut kinds = [MediaKind::Image; 18];
    let mut i = 0;
    while i < TYPES.len() {
        kinds[i] = TYPES[i].0;
        i += 1;
    }
    kinds
};

/// The regions anybody can read, in order: after the ROM's own (`regions_for`).
const REGIONS: [&str; 8] = ["wor", "us", "eu", "ss", "uk", "fr", "de", "jp"];

/// ScreenScraper's regions in the ROM's name (`Sonic (Europe).md`, `(USA, Europe)`), in the
/// order they come, then the usual ones: a European copy gets its European name, box and date.
pub(super) fn regions_for(stem: &str) -> Vec<&'static str> {
    const TAGS: [(&str, &[&str]); 17] = [
        ("World", &["wor"]),
        ("USA", &["us"]),
        ("Europe", &["eu"]),
        ("Japan", &["jp"]),
        ("UK", &["uk", "eu"]),
        ("France", &["fr", "eu"]),
        ("Germany", &["de", "eu"]),
        ("Spain", &["es", "eu"]),
        ("Italy", &["it", "eu"]),
        ("Netherlands", &["nl", "eu"]),
        ("Sweden", &["se", "eu"]),
        ("Portugal", &["pt", "eu"]),
        ("Brazil", &["br"]),
        ("Canada", &["ca", "us"]),
        ("Australia", &["au", "eu"]),
        ("Korea", &["kr"]),
        ("Asia", &["asi"]),
    ];
    let mut order: Vec<&'static str> = Vec::new();
    let tags = stem
        .split(['(', ')', ','])
        .map(str::trim)
        .filter_map(|word| TAGS.iter().find(|(tag, _)| *tag == word));
    for code in tags
        .flat_map(|(_, codes)| codes.iter())
        .chain(REGIONS.iter())
    {
        if !order.contains(code) {
            order.push(code);
        }
    }
    order
}

impl Source for ScreenScraper {
    fn id(&self) -> &str {
        "screenscraper"
    }
    fn name(&self) -> &str {
        "ScreenScraper"
    }
    fn about(&self) -> &str {
        if HAS_BUILT_IN {
            "Every kind of media, matched by the ROM's hash. Your own screenscraper.fr login \
             raises how many games a day"
        } else {
            "Every kind of media, matched by the ROM's hash. Needs a developer ID and password \
             (ask for them on screenscraper.fr); your own login raises how many games a day"
        }
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
        Ok(ask(http, credentials, query)?.and_then(|game| parse(&game, query.stem)))
    }
}

/// The developer account and the user's login, as a URL's parameters.
fn account(credentials: &Credentials) -> String {
    let get = |k: &str| credentials.get(k).map_or("", |v| v.trim());
    let (dev_id, dev_password) = developer(
        get("devId"),
        get("devPassword"),
        HAS_BUILT_IN.then(|| (unmask(&MASKED_ID), unmask(&MASKED_PASSWORD))),
    );
    let mut account = format!(
        "devid={}&devpassword={}&softname=Forma",
        encode(&dev_id),
        encode(&dev_password)
    );
    if !get("user").is_empty() {
        account.push_str(&format!(
            "&ssid={}&sspassword={}",
            encode(get("user")),
            encode(get("password"))
        ));
    }
    account
}

/// ScreenScraper's entry for the game, if it knows it.
fn ask(
    http: &dyn Http,
    credentials: &Credentials,
    query: &Query<'_>,
) -> Result<Option<Value>, LookupError> {
    let mut url = format!(
        "https://api.screenscraper.fr/api2/jeuInfos.php?{}&output=json&romtype=rom&romnom={}",
        account(credentials),
        encode(query.file_name)
    );
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
    let Ok(mut doc) = serde_json::from_slice::<Value>(&body) else {
        let text = String::from_utf8_lossy(&body);
        if text.to_lowercase().contains("login") {
            return Err(LookupError::Credentials(
                "ScreenScraper refused the username or password".into(),
            ));
        }
        return Ok(None);
    };
    Ok(Some(doc["response"]["jeu"].take()).filter(|game| !game.is_null()))
}

/// The text of the entry for the first region (or language) of `order` there is, and which
/// region that was.
fn by_region_noted(list: &Value, key: &str, order: &[&str]) -> Option<(String, Option<String>)> {
    let items = list.as_array()?;
    let item = order
        .iter()
        .find_map(|r| items.iter().find(|i| i[key].as_str() == Some(r)))
        .or_else(|| items.first())?;
    Some((
        json_text(item.get("text"))?,
        item[key].as_str().map(str::to_owned),
    ))
}

/// Every different name, the preferred region's first, with the regions that use it in words.
fn names_by_region(list: &Value, regions: &[&str]) -> Vec<(String, Option<String>)> {
    let mut items: Vec<&Value> = list
        .as_array()
        .map(|l| l.iter().collect())
        .unwrap_or_default();
    items.sort_by_key(|i| {
        regions
            .iter()
            .position(|r| i["region"].as_str() == Some(r))
            .unwrap_or(usize::MAX)
    });
    let mut names: Vec<(String, Vec<String>)> = Vec::new();
    for item in items {
        let Some(text) = json_text(item.get("text")) else {
            continue;
        };
        let region = item["region"].as_str().and_then(region_name);
        let at = names.iter().position(|(name, _)| *name == text);
        let at = at.unwrap_or_else(|| {
            names.push((text, Vec::new()));
            names.len() - 1
        });
        names[at].1.extend(region);
    }
    names
        .into_iter()
        .map(|(name, regions)| (name, (!regions.is_empty()).then(|| regions.join(" · "))))
        .collect()
}

fn by_region(list: &Value, key: &str, order: &[&str]) -> Option<String> {
    by_region_noted(list, key, order).map(|(text, _)| text)
}

/// A ScreenScraper region in words. `ss` is its own catch-all, so no country at all.
fn region_name(code: &str) -> Option<String> {
    let name = match code {
        "ss" | "" => return None,
        "wor" => "World",
        "us" => "USA",
        "eu" => "Europe",
        "jp" => "Japan",
        "uk" => "UK",
        "fr" => "France",
        "de" => "Germany",
        "es" => "Spain",
        "it" => "Italy",
        "pt" => "Portugal",
        "nl" => "Netherlands",
        "se" => "Sweden",
        "br" => "Brazil",
        "ca" => "Canada",
        "au" => "Australia",
        "kr" => "Korea",
        "cn" => "China",
        "tw" => "Taiwan",
        "ru" => "Russia",
        other => return Some(other.to_uppercase()),
    };
    Some(name.to_owned())
}

fn language_name(code: &str) -> Option<String> {
    let name = match code {
        "" => return None,
        "en" => "English",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "it" => "Italian",
        "pt" => "Portuguese",
        "ja" | "jp" => "Japanese",
        other => return Some(other.to_uppercase()),
    };
    Some(name.to_owned())
}

/// What sets a type of media apart from the others of its kind.
fn variant(media_type: &str) -> Option<String> {
    let name = match media_type {
        "video-normalized" => "Small file",
        "video" => "Full size",
        "wheel-hd" => "HD",
        "wheel-carbon" => "Carbon",
        "wheel-steel" => "Steel",
        "screenmarqueesmall" => "Small",
        "support-texture" => "Flat texture",
        _ => return None,
    };
    Some(name.to_owned())
}

/// The game ScreenScraper answered, for the ROM `stem`: what is shown first (its name, release
/// date and media) is the ROM's own region's, where there is one ([`regions_for`]).
pub(super) fn parse(game: &Value, stem: &str) -> Option<Found> {
    if !game.is_object() {
        return None;
    }
    let regions = regions_for(stem);
    let mut names = names_by_region(&game["noms"], &regions).into_iter();
    let name = names.next();
    let description = by_region_noted(&game["synopsis"], "langue", &["en", "fr", "de", "es"]);
    let released = by_region_noted(&game["dates"], "region", &regions);
    let genre = game["genres"].as_array().and_then(|genres| {
        let principal = genres
            .iter()
            .find(|g| g["principale"].as_str() == Some("1"))
            .or_else(|| genres.first())?;
        by_region(&principal["noms"], "langue", &["en", "fr"])
    });
    let metadata = Metadata {
        name: name.as_ref().map(|(text, _)| text.clone()),
        description: description.as_ref().map(|(text, _)| text.clone()),
        developer: json_text(game["developpeur"].get("text")),
        publisher: json_text(game["editeur"].get("text")),
        genre,
        players: json_text(game["joueurs"].get("text")),
        release_date: released.as_ref().and_then(|(d, _)| date(d)),
        rating: json_number(game["note"].get("text")).map(|n| n / 20.0),
        age_rating: age_ratings(
            game["classifications"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|c| Some((c["type"].as_str()?, c["text"].as_str()?))),
        ),
    };
    let medias = game["medias"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut media = Vec::new();
    let mut notes = Notes::default();
    let tag = |found: &Option<(String, Option<String>)>| found.as_ref()?.1.clone();
    notes.names = names.collect();
    for (field, note) in [
        (
            "name",
            name.as_ref().and_then(|(_, regions)| regions.clone()),
        ),
        (
            "description",
            tag(&description).as_deref().and_then(language_name),
        ),
        (
            "releaseDate",
            tag(&released).as_deref().and_then(region_name),
        ),
    ] {
        if let Some(note) = note {
            notes.text.insert(field, note);
        }
    }
    for (kind, types) in TYPES {
        for t in types {
            let mut of_type: Vec<&Value> = medias
                .iter()
                .filter(|m| m["type"].as_str() == Some(t))
                .collect();
            of_type.sort_by_key(|m| {
                regions
                    .iter()
                    .position(|r| m["region"].as_str() == Some(r))
                    .unwrap_or(regions.len())
            });
            for m in of_type {
                if let Some(url) = m["url"].as_str() {
                    notes.media.insert(
                        url.to_owned(),
                        MediaNote {
                            region: m["region"].as_str().and_then(region_name),
                            variant: variant(t),
                            format: m["format"].as_str().map(str::to_owned),
                            size: m["size"]
                                .as_u64()
                                .or_else(|| m["size"].as_str()?.parse().ok()),
                        },
                    );
                    media.push((kind, url.to_owned()));
                }
            }
        }
    }
    Some(Found {
        metadata,
        media,
        notes,
    })
}
