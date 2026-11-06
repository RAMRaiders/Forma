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
/// Age ratings, `BOARD:RATING` comma-separated, e.g. `ESRB:M,PEGI:18` (Forma's own tag).
pub const AGE_RATING: &str = "age_rating";
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
pub const HASH_CRC32: &str = "hash.crc32";
pub const HASH_MD5: &str = "hash.md5";
pub const HASH_SHA1: &str = "hash.sha1";

/// Media fields share a prefix; the part after it is the media kind a theme asks for.
pub const MEDIA_PREFIX: &str = "media.";
pub const MEDIA_IMAGE: &str = "media.image";
pub const MEDIA_THUMBNAIL: &str = "media.thumbnail";
pub const MEDIA_MARQUEE: &str = "media.marquee";
pub const MEDIA_VIDEO: &str = "media.video";
pub const MEDIA_FANART: &str = "media.fanart";
pub const MEDIA_MANUAL: &str = "media.manual";
pub const MEDIA_BOXBACK: &str = "media.boxback";
/// The box's spine, upright (Forma's own tag: ES has none).
pub const MEDIA_BOXSIDE: &str = "media.boxside";
/// The whole cover laid flat: back, spine and front (Forma's own).
pub const MEDIA_BOXFULL: &str = "media.boxfull";
/// A wide banner (Forma's own).
pub const MEDIA_BANNER: &str = "media.banner";
/// An arcade flyer (Forma's own).
pub const MEDIA_FLYER: &str = "media.flyer";
/// An arcade cabinet (Forma's own).
pub const MEDIA_CABINET: &str = "media.cabinet";
/// A 3D box (ES's "alt boxart").
pub const MEDIA_BOXART: &str = "media.boxart";
/// A clear logo.
pub const MEDIA_WHEEL: &str = "media.wheel";
pub const MEDIA_TITLESHOT: &str = "media.titleshot";
pub const MEDIA_CARTRIDGE: &str = "media.cartridge";
pub const MEDIA_MAP: &str = "media.map";
pub const MEDIA_BEZEL: &str = "media.bezel";
/// A composite of screenshot, box and logo.
pub const MEDIA_MIX: &str = "media.mix";
