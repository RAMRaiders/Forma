//! Metadata field names shared by every source (gamelist, external import, the user) and the
//! library database. Values are stored as normalised strings: dates as ISO 8601, booleans as
//! `0`/`1`, ratings as `0.0`–`1.0`, media as absolute paths.

pub const NAME: &str = "name";
pub const DESCRIPTION: &str = "description";
pub const DEVELOPER: &str = "developer";
pub const PUBLISHER: &str = "publisher";
pub const GENRE: &str = "genre";
pub const PLAYERS: &str = "players";
pub const REGION: &str = "region";
pub const LANG: &str = "lang";
pub const RELEASE_DATE: &str = "release_date";
pub const RATING: &str = "rating";
pub const HIDDEN: &str = "hidden";
pub const FAVORITE: &str = "favorite";
pub const PLAY_COUNT: &str = "play_count";
pub const LAST_PLAYED: &str = "last_played";
/// Total seconds played.
pub const GAME_TIME: &str = "game_time";
pub const RA_GAME_ID: &str = "ra_game_id";
pub const RA_HASH: &str = "ra_hash";

/// Media fields share a prefix; the part after it is the media kind a theme asks for.
pub const MEDIA_PREFIX: &str = "media.";
pub const MEDIA_IMAGE: &str = "media.image";
pub const MEDIA_THUMBNAIL: &str = "media.thumbnail";
pub const MEDIA_MARQUEE: &str = "media.marquee";
pub const MEDIA_VIDEO: &str = "media.video";
pub const MEDIA_FANART: &str = "media.fanart";
pub const MEDIA_MANUAL: &str = "media.manual";
