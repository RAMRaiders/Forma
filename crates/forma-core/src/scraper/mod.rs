//! The scraper: metadata and media for games, from every source the user turned on, written
//! where EmulationStation writes them (see `docs/scraper.md`).
//!
//! Each [`Source`] answers a [`Query`] with what it found. Sources are asked in the user's
//! order, and for each field and each kind of media the first answer wins; a source is not
//! asked once everything wanted is found. Forma downloads the media itself, into the system's
//! `images`, `videos` and `manuals` folders, and edits the game's entry in `gamelist.xml` in
//! place. The library then re-imports it through the watcher, as for any gamelist change.

pub mod duplicates;
mod engine;
pub(crate) use text::title_key;
pub mod pick;
pub mod platforms;
pub mod regions;
mod sources;
pub(crate) mod text;

use std::borrow::Cow;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use engine::{Outcome, scrape_game};

/// A date as a source gives it, as `YYYY[-MM[-DD]]`.
pub fn text_date(text: &str) -> Option<String> {
    text::date(text)
}
pub use sources::builtin;

/// Builds LaunchBox's data again when an older Forma built it; `Ok(true)` when it did.
pub fn upgrade_launchbox(
    http: &dyn crate::net::Http,
    paths: &crate::paths::Paths,
) -> Result<bool, String> {
    sources::launchbox::upgrade(http, &paths.cache_dir().join("launchbox.db"))
}

use crate::net::Http;

/// Where the user's choices are kept (JSON, a [`Config`]).
pub const CONFIG_SETTING: &str = "scraper.config";
/// Where the credentials are kept (JSON: source id → key → value).
pub const CREDENTIALS_SETTING: &str = "scraper.credentials";

/// A kind of media, as EmulationStation names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    /// A screenshot.
    Image,
    /// The box front.
    Thumbnail,
    Boxback,
    /// The box's spine, upright (Forma's own tag: ES has none).
    Boxside,
    /// The whole box cover laid flat: back, spine and front in one (Forma's own).
    Boxfull,
    /// A 3D box.
    Boxart,
    Marquee,
    /// A wide banner, as stores show (Forma's own).
    Banner,
    /// A clear logo.
    Wheel,
    Fanart,
    Titleshot,
    Cartridge,
    Map,
    Bezel,
    /// Screenshot, box and logo in one picture.
    Mix,
    Video,
    Manual,
    /// An arcade game's flyer: its "box" (Forma's own).
    Flyer,
    /// An arcade cabinet (Forma's own).
    Cabinet,
}

impl MediaKind {
    pub const ALL: [Self; 19] = [
        Self::Thumbnail,
        Self::Image,
        Self::Titleshot,
        Self::Marquee,
        Self::Wheel,
        Self::Boxback,
        Self::Boxside,
        Self::Boxfull,
        Self::Boxart,
        Self::Banner,
        Self::Fanart,
        Self::Cartridge,
        Self::Map,
        Self::Bezel,
        Self::Mix,
        Self::Flyer,
        Self::Cabinet,
        Self::Video,
        Self::Manual,
    ];

    /// The `gamelist.xml` tag, and the library's media key.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Thumbnail => "thumbnail",
            Self::Boxback => "boxback",
            Self::Boxside => "boxside",
            Self::Boxfull => "boxfull",
            Self::Boxart => "boxart",
            Self::Banner => "banner",
            Self::Flyer => "flyer",
            Self::Cabinet => "cabinet",
            Self::Marquee => "marquee",
            Self::Wheel => "wheel",
            Self::Fanart => "fanart",
            Self::Titleshot => "titleshot",
            Self::Cartridge => "cartridge",
            Self::Map => "map",
            Self::Bezel => "bezel",
            Self::Mix => "mix",
            Self::Video => "video",
            Self::Manual => "manual",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "Screenshot",
            Self::Thumbnail => "Box",
            Self::Boxback => "Box back",
            Self::Boxside => "Box spine",
            Self::Boxfull => "Box, full cover",
            Self::Boxart => "3D box",
            Self::Banner => "Banner",
            Self::Flyer => "Flyer (arcade)",
            Self::Cabinet => "Cabinet (arcade)",
            Self::Marquee => "Marquee",
            Self::Wheel => "Logo (wheel)",
            Self::Fanart => "Fan art",
            Self::Titleshot => "Title screen",
            Self::Cartridge => "Cartridge or disc",
            Self::Map => "Map",
            Self::Bezel => "Bezel",
            Self::Mix => "Mix",
            Self::Video => "Video",
            Self::Manual => "Manual",
        }
    }

    /// The folder under the system's ROM folder, as ES uses.
    pub(crate) fn folder(self) -> &'static str {
        match self {
            Self::Video => "videos",
            Self::Manual => "manuals",
            _ => "images",
        }
    }

    /// The file name suffix ES gives it: `<rom>-<suffix>.<ext>`.
    pub(crate) fn suffix(self) -> &'static str {
        match self {
            Self::Thumbnail => "thumb",
            Self::Boxart => "box",
            other => other.tag(),
        }
    }

    /// The largest file accepted.
    pub(crate) fn limit(self) -> u64 {
        match self {
            Self::Video | Self::Manual => 200 << 20,
            _ => 25 << 20,
        }
    }
}

/// Text about a game. Empty fields were not found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub genre: Option<String>,
    pub players: Option<String>,
    /// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`.
    pub release_date: Option<String>,
    /// From 0 to 1.
    pub rating: Option<f64>,
    /// `BOARD:RATING` comma-separated, e.g. `ESRB:M,PEGI:18`.
    pub age_rating: Option<String>,
}

impl Metadata {
    /// HTML's character codes in its words as the characters they stand for (`&quot;` as `"`).
    pub(crate) fn decode_entities(&mut self) {
        for value in [
            &mut self.name,
            &mut self.description,
            &mut self.developer,
            &mut self.publisher,
            &mut self.genre,
        ]
        .into_iter()
        .flatten()
        {
            *value = text::decode_entities(value);
        }
    }

    /// Fills what is empty here from `other`.
    pub(crate) fn fill_from(&mut self, other: Self) {
        fn take<T>(mine: &mut Option<T>, theirs: Option<T>) {
            if mine.is_none() {
                *mine = theirs;
            }
        }
        take(&mut self.name, other.name);
        take(&mut self.description, other.description);
        take(&mut self.developer, other.developer);
        take(&mut self.publisher, other.publisher);
        take(&mut self.genre, other.genre);
        take(&mut self.players, other.players);
        take(&mut self.release_date, other.release_date);
        take(&mut self.rating, other.rating);
        take(&mut self.age_rating, other.age_rating);
    }

    pub(crate) fn is_complete(&self, age_rating: bool) -> bool {
        self.name.is_some()
            && self.description.is_some()
            && self.developer.is_some()
            && self.publisher.is_some()
            && self.genre.is_some()
            && self.players.is_some()
            && self.release_date.is_some()
            && self.rating.is_some()
            && (!age_rating || self.age_rating.is_some())
    }
}

/// What a source found for a game: its text, and media by kind, best first.
/// What a source says about one picture, video or manual, to tell it from the others.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaNote {
    /// The country or region it is for, in words.
    pub region: Option<String>,
    /// What sets it apart from others of its kind: `Small file`.
    pub variant: Option<String>,
    pub format: Option<String>,
    pub size: Option<u64>,
}

/// What a source knows about what it found, beyond the values themselves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notes {
    /// By address.
    pub media: std::collections::HashMap<String, MediaNote>,
    /// By field (`name`, `releaseDate`): the country or language of its value.
    pub text: std::collections::HashMap<&'static str, String>,
    /// The game's other names, with the regions each is used in.
    pub names: Vec<(String, Option<String>)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub metadata: Metadata,
    pub media: Vec<(MediaKind, String)>,
    pub notes: Notes,
}

/// A ROM's hashes, lower-case hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hashes {
    pub crc32: String,
    pub md5: String,
    pub sha1: String,
}

/// What a source is asked about.
#[derive(Debug, Clone)]
pub struct Query<'a> {
    /// The Batocera system, e.g. `snes`.
    pub system: &'a str,
    /// Its `<platform>` names from `es_systems.cfg`, e.g. `["snes"]`.
    pub platforms: &'a [String],
    /// The ROM's file name, e.g. `Super Metroid (USA).sfc`.
    pub file_name: &'a str,
    /// The file name without its extension: the No-Intro name, for most sets.
    pub stem: &'a str,
    /// A name to search with: the game's, without tags such as `(USA)`.
    pub name: &'a str,
    pub size: u64,
    /// When the file was small enough to hash.
    pub hashes: Option<&'a Hashes>,
    /// The media kinds still wanted, so a source can skip calls for the rest.
    pub wanted: &'a [MediaKind],
    /// The regions whose names, boxes and dates to prefer, best first ([`regions::order`]).
    pub regions: &'a [String],
    pub wants_metadata: bool,
}

/// Why a source gave no answer, other than not knowing the game.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LookupError {
    /// The source refused the credentials; the user must fix them.
    #[error("{0}")]
    Credentials(String),
    /// Over the source's quota for now; it is not asked again this run.
    #[error("{0}")]
    Quota(String),
    #[error("{0}")]
    Failed(String),
}

/// Something the user enters for a source: an API key, a login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub key: Cow<'static, str>,
    pub label: Cow<'static, str>,
    pub secret: bool,
    /// Needed for the source to be asked at all.
    pub required: bool,
}

impl Credential {
    pub const fn new(key: &'static str, label: &'static str, secret: bool, required: bool) -> Self {
        Self {
            key: Cow::Borrowed(key),
            label: Cow::Borrowed(label),
            secret,
            required,
        }
    }
}

/// The user's credentials for one source.
pub type Credentials = BTreeMap<String, String>;

/// A place metadata and media come from.
pub trait Source: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    /// A line about it for the menu: what it needs and where to get it.
    fn about(&self) -> &str;
    /// What the user enters for it.
    fn needs(&self) -> &[Credential] {
        &[]
    }
    /// The kinds of media it can give.
    fn gives(&self) -> &[MediaKind];
    /// Whether it has metadata, not only media.
    fn has_metadata(&self) -> bool {
        true
    }
    /// Whether it can know games of this system at all.
    fn covers(&self, query: &Query<'_>) -> bool {
        let _ = query;
        true
    }
    /// What it knows about the game, or `None` when it does not know it.
    fn lookup(
        &self,
        http: &dyn Http,
        credentials: &Credentials,
        query: &Query<'_>,
    ) -> Result<Option<Found>, LookupError>;

    /// Whether `credentials` has every required one.
    fn ready(&self, credentials: &Credentials) -> bool {
        self.needs().iter().filter(|c| c.required).all(|c| {
            credentials
                .get(c.key.as_ref())
                .is_some_and(|v| !v.trim().is_empty())
        })
    }
}

/// One source in the user's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceChoice {
    pub id: String,
    pub enabled: bool,
}

/// What the user chose: sources in order, their region profile, and what to do with games
/// already scraped. What to fetch is the theme's ([`Needs`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub sources: Vec<SourceChoice>,
    /// Replace what a game already has; otherwise only fill what is missing.
    pub replace: bool,
    /// Leave a game that has been scraped (a description or any media) as it is.
    pub skip_scraped: bool,
    /// The ROM's own regions (from its file name) come before `regions`.
    pub own_region: bool,
    /// The regions to prefer, best first: names, boxes and dates of the first one there is.
    pub regions: Vec<String>,
    /// Download manuals, when the theme draws them: they are large.
    pub manuals: bool,
    /// What the theme in use asks for; set for a run, not stored.
    #[serde(skip)]
    pub needs: Needs,
}

/// What a run fetches for a theme that does not say what it draws.
pub const DEFAULT_MEDIA: [MediaKind; 5] = [
    MediaKind::Thumbnail,
    MediaKind::Image,
    MediaKind::Marquee,
    MediaKind::Wheel,
    MediaKind::Video,
];

/// What the theme in use asks the scraper for: it decides what is fetched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Needs {
    /// What it asks for, on top of what it draws (or of [`DEFAULT_MEDIA`]).
    pub media: Vec<MediaKind>,
    /// Every kind it draws, backups among them; empty when it does not say.
    pub shows: Vec<MediaKind>,
    /// Kinds it draws only when another is missing: the kind, and the one it stands in for.
    pub backups: Vec<(MediaKind, MediaKind)>,
    /// The game's age ratings (`ageRating`): not otherwise wanted.
    pub age_rating: bool,
}

impl Needs {
    /// From a theme's `scraper` block: what it always needs, and what `view`, the view in use,
    /// needs too. What Forma does not know is left out.
    pub fn from_theme(asked: &crate::themes::ThemeScraper, view: Option<&str>) -> Self {
        let extra = view.and_then(|v| asked.views.get(v));
        let media = known_kinds(
            asked
                .media
                .iter()
                .chain(extra.iter().flat_map(|e| e.media.iter())),
        );
        let mut fields = asked
            .fields
            .iter()
            .chain(extra.iter().flat_map(|e| e.fields.iter()));
        let backups = asked
            .backups
            .iter()
            .filter_map(|(kind, main)| {
                let pair = known_kinds([kind, main]);
                (pair.len() == 2).then(|| (pair[0], pair[1]))
            })
            .collect();
        Self {
            media,
            shows: known_kinds(&asked.shows),
            backups,
            age_rating: fields.any(|f| f == "ageRating"),
        }
    }
}

/// The kinds Forma knows among `names`, in order and once each.
pub fn known_kinds<'a>(names: impl IntoIterator<Item = &'a String>) -> Vec<MediaKind> {
    let mut kinds = Vec::new();
    for name in names {
        if let Ok(kind) =
            serde_json::from_value::<MediaKind>(serde_json::Value::String(name.clone()))
            && !kinds.contains(&kind)
        {
            kinds.push(kind);
        }
    }
    kinds
}

impl Config {
    /// The media kinds to fetch: what the theme draws (else [`DEFAULT_MEDIA`]), but not its
    /// backups, which come only when what they stand in for cannot be had; then what it asks for.
    pub fn all_media(&self) -> Vec<MediaKind> {
        let needs = &self.needs;
        let mut all: Vec<MediaKind> = if needs.shows.is_empty() {
            DEFAULT_MEDIA.to_vec()
        } else {
            needs
                .shows
                .iter()
                .filter(|k| !needs.backups.iter().any(|(b, _)| b == *k))
                .copied()
                .collect()
        };
        for kind in &needs.media {
            if !all.contains(kind) && !needs.backups.iter().any(|(b, _)| b == kind) {
                all.push(*kind);
            }
        }
        if !self.manuals {
            all.retain(|k| *k != MediaKind::Manual);
        }
        all
    }

    /// Whether the age rating is wanted: the theme asks for it.
    pub fn wants_age_rating(&self) -> bool {
        self.needs.age_rating
    }

    /// The regions to look for with the ROM `stem`, best first.
    pub fn regions_for(&self, stem: &str) -> Vec<String> {
        regions::order(stem, self.own_region, &self.regions)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            replace: false,
            skip_scraped: false,
            own_region: true,
            regions: regions::default_order(),
            manuals: true,
            needs: Needs::default(),
        }
    }
}

impl Config {
    /// The stored config (or the default), with `sources` listing every one of `available`:
    /// the user's in their order, then any new ones, on.
    pub fn load(stored: Option<&str>, available: &[Box<dyn Source>]) -> Self {
        let mut config: Self = stored
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        config
            .sources
            .retain(|c| available.iter().any(|s| s.id() == c.id));
        let mut seen = std::collections::HashSet::new();
        config.sources.retain(|c| seen.insert(c.id.clone()));
        for source in available {
            if !config.sources.iter().any(|c| c.id == source.id()) {
                config.sources.push(SourceChoice {
                    id: source.id().to_owned(),
                    enabled: true,
                });
            }
        }
        config
            .regions
            .retain(|r| regions::REGIONS.contains(&r.as_str()));
        config
    }
}

/// Every source's credentials, from [`CREDENTIALS_SETTING`].
pub fn load_credentials(stored: Option<&str>) -> BTreeMap<String, Credentials> {
    stored
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
