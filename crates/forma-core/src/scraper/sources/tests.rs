use std::sync::Mutex;

use serde_json::json;

use super::super::{Credentials, Hashes, LookupError, MediaKind, Query, Source};
use super::*;
use crate::net::{Http, HttpError, HttpRequest};

/// Answers by URL prefix, in order; records every request.
struct Api {
    answers: Vec<(&'static str, Result<Vec<u8>, u16>)>,
    sent: Mutex<Vec<HttpRequest>>,
}

impl Api {
    fn new(answers: Vec<(&'static str, Result<serde_json::Value, u16>)>) -> Self {
        Self {
            answers: answers
                .into_iter()
                .map(|(p, a)| (p, a.map(|v| serde_json::to_vec(&v).unwrap())))
                .collect(),
            sent: Mutex::new(Vec::new()),
        }
    }
    fn urls(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.url.clone())
            .collect()
    }
}

impl Http for Api {
    fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        self.send(&HttpRequest::get(url))
    }
    fn send(&self, request: &HttpRequest) -> Result<Vec<u8>, HttpError> {
        self.sent.lock().unwrap().push(request.clone());
        match self
            .answers
            .iter()
            .find(|(p, _)| request.url.starts_with(p))
        {
            Some((_, Ok(body))) => Ok(body.clone()),
            Some((_, Err(status))) => Err(HttpError {
                status: Some(*status),
                message: format!("HTTP {status}"),
            }),
            None => Err(HttpError {
                status: Some(404),
                message: "no route".into(),
            }),
        }
    }
}

const ALL: [MediaKind; 19] = MediaKind::ALL;

/// The regions a ROM called `stem` is looked for in, by the default profile.
fn regions(stem: &str) -> &'static [String] {
    Box::leak(
        crate::scraper::regions::order(stem, true, &crate::scraper::regions::default_order())
            .into_boxed_slice(),
    )
}

/// ScreenScraper's answer read for the ROM `stem`, by the default profile.
fn ss_parse(game: &serde_json::Value, stem: &str) -> Option<crate::scraper::Found> {
    super::screenscraper::parse(game, regions(stem))
}

fn query<'a>(system: &'a str, platforms: &'a [String], hashes: Option<&'a Hashes>) -> Query<'a> {
    Query {
        system,
        platforms,
        file_name: "Super Metroid (USA).sfc",
        stem: "Super Metroid (USA)",
        name: "Super Metroid",
        size: 3_145_728,
        hashes,
        wanted: &ALL,
        wants_metadata: true,
        regions: regions("Super Metroid (USA)"),
    }
}

fn creds(pairs: &[(&str, &str)]) -> Credentials {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn find(id: &str) -> Box<dyn Source> {
    builtin().into_iter().find(|s| s.id() == id).unwrap()
}

#[test]
fn screenscraper_matches_by_hash_and_picks_regions() {
    let snes = vec!["snes".to_owned()];
    let hashes = Hashes {
        crc32: "d63ed5f8".into(),
        md5: "21f3e98d".into(),
        sha1: "da957f0d".into(),
    };
    let api = Api::new(vec![(
        "https://api.screenscraper.fr/api2/jeuInfos.php",
        Ok(json!({"response": {"jeu": {
            "noms": [{"region": "jp", "text": "Super Metroid (J)"}, {"region": "us", "text": "Super Metroid"}],
            "synopsis": [{"langue": "fr", "text": "Samus..."}, {"langue": "en", "text": "Samus returns"}],
            "developpeur": {"id": "1", "text": "Nintendo R&D1"},
            "editeur": {"id": "2", "text": "Nintendo"},
            "joueurs": {"text": "1"},
            "note": {"text": "18"},
            "classifications": [{"type": "SS", "text": "12"}, {"type": "PEGI", "text": "7"},
                                {"type": "ESRB", "text": "E"}],
            "dates": [{"region": "jp", "text": "1994-03-19"}, {"region": "us", "text": "1994-04-18"}],
            "genres": [{"principale": "0", "noms": [{"langue": "en", "text": "Platform"}]},
                       {"principale": "1", "noms": [{"langue": "en", "text": "Action"}]}],
            "medias": [
                {"type": "box-2D", "region": "jp", "url": "https://ss/box-jp"},
                {"type": "box-2D", "region": "us", "url": "https://ss/box-us"},
                {"type": "wheel", "region": "wor", "url": "https://ss/wheel"},
                {"type": "wheel-hd", "region": "wor", "url": "https://ss/wheel-hd"},
                {"type": "video-normalized", "url": "https://ss/video"},
                {"type": "manuel", "region": "us", "url": "https://ss/manual"},
                {"type": "mixrbv2", "url": "https://ss/mix"},
                {"type": "box-2D-side", "region": "us", "url": "https://ss/spine"},
                {"type": "box-texture", "region": "us", "url": "https://ss/full"},
                {"type": "steamgrid", "url": "https://ss/banner"},
                {"type": "flyer", "region": "wor", "url": "https://ss/flyer"}
            ]
        }}})),
    )]);
    let c = creds(&[
        ("devId", "dev"),
        ("devPassword", "p&w"),
        ("user", "me"),
        ("password", "secret"),
    ]);
    let found = find("screenscraper")
        .lookup(&api, &c, &query("snes", &snes, Some(&hashes)))
        .unwrap()
        .unwrap();
    let url = &api.urls()[0];
    for part in [
        "devid=dev",
        "devpassword=p%26w",
        "ssid=me",
        "sspassword=secret",
        "systemeid=4",
        "romnom=Super%20Metroid%20%28USA%29.sfc",
        "romtaille=3145728",
        "crc=d63ed5f8",
        "sha1=da957f0d",
        "output=json",
    ] {
        assert!(url.contains(part), "{part} in {url}");
    }
    let m = &found.metadata;
    assert_eq!(m.name.as_deref(), Some("Super Metroid"));
    assert_eq!(m.description.as_deref(), Some("Samus returns"));
    assert_eq!(m.genre.as_deref(), Some("Action"));
    assert_eq!(m.release_date.as_deref(), Some("1994-04-18"));
    assert_eq!(m.rating, Some(0.9));
    // Known boards only, ESRB first.
    assert_eq!(m.age_rating.as_deref(), Some("ESRB:E,PEGI:7"));
    let urls = |k| {
        found
            .media
            .iter()
            .filter(|(m, _)| *m == k)
            .map(|(_, u)| u.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        urls(MediaKind::Thumbnail),
        ["https://ss/box-us", "https://ss/box-jp"]
    );
    assert_eq!(
        urls(MediaKind::Wheel),
        ["https://ss/wheel-hd", "https://ss/wheel"]
    );
    assert_eq!(urls(MediaKind::Manual), ["https://ss/manual"]);
    assert_eq!(urls(MediaKind::Mix), ["https://ss/mix"]);
    // Forma's own kinds: the spine, the full cover, the banner, the flyer.
    assert_eq!(urls(MediaKind::Boxside), ["https://ss/spine"]);
    assert_eq!(urls(MediaKind::Boxfull), ["https://ss/full"]);
    assert_eq!(urls(MediaKind::Banner), ["https://ss/banner"]);
    assert_eq!(urls(MediaKind::Flyer), ["https://ss/flyer"]);
    let gives = find("screenscraper").gives().to_vec();
    assert!(gives.contains(&MediaKind::Boxside) && !gives.contains(&MediaKind::Cabinet));
}

#[test]
fn screenscraper_uses_the_built_in_account_unless_the_user_entered_a_whole_one() {
    use super::screenscraper::developer;
    let built_in = || Some(("forma".to_owned(), "built".to_owned()));
    let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
    assert_eq!(developer("", "", built_in()), pair("forma", "built"));
    assert_eq!(developer("mine", "", built_in()), pair("forma", "built"));
    assert_eq!(developer("mine", "pw", built_in()), pair("mine", "pw"));
    assert_eq!(developer("mine", "", None), pair("mine", ""));
}

#[test]
fn screenscraper_errors_say_what_to_fix() {
    let snes = vec!["snes".to_owned()];
    let c = creds(&[("devId", "d"), ("devPassword", "p")]);
    let ss = find("screenscraper");
    for (status, want) in [(430, "quota"), (403, "creds"), (404, "none")] {
        let api = Api::new(vec![("https://api.screenscraper.fr", Err(status))]);
        let got = ss.lookup(&api, &c, &query("snes", &snes, None));
        match (want, got) {
            ("quota", Err(LookupError::Quota(_)))
            | ("creds", Err(LookupError::Credentials(_)))
            | ("none", Ok(None)) => {}
            (w, g) => panic!("{status}: wanted {w}, got {g:?}"),
        }
    }
    // A build with its own developer account needs nothing more from the user.
    assert_eq!(
        ss.ready(&creds(&[("user", "me")])),
        super::screenscraper::HAS_BUILT_IN
    );
    assert!(ss.ready(&c));
}

#[test]
fn thegamesdb_searches_by_platform_and_names_ids() {
    let snes = vec!["snes".to_owned()];
    let api = Api::new(vec![
        (
            "https://api.thegamesdb.net/v1.1/Games/ByGameName",
            Ok(json!({"data": {"games": [
                {"id": 9, "game_title": "Super Metroid: Redesign"},
                {"id": 7, "game_title": "Super Metroid", "release_date": "1994-03-19", "players": 1,
                 "overview": "Samus", "rating": "E - Everyone", "developers": [3], "publishers": [4], "genres": [1, 15]}
            ]}})),
        ),
        (
            "https://api.thegamesdb.net/v1/Games/Images",
            Ok(json!({"data": {
                "base_url": {"original": "https://cdn/o/"},
                "images": {"7": [
                    {"type": "boxart", "side": "back", "filename": "boxart/back/7-1.jpg"},
                    {"type": "boxart", "side": "front", "filename": "boxart/front/7-1.jpg"},
                    {"type": "banner", "filename": "graphical/7-g.jpg"},
                    {"type": "clearlogo", "filename": "clearlogo/7.png"}
                ]}
            }})),
        ),
        (
            "https://api.thegamesdb.net/v1/Genres",
            Ok(
                json!({"data": {"genres": {"1": {"id": 1, "name": "Action"}, "15": {"id": 15, "name": "Platform"}}}}),
            ),
        ),
        (
            "https://api.thegamesdb.net/v1/Developers",
            Ok(json!({"data": {"developers": {"3": {"id": 3, "name": "Nintendo R&D1"}}}})),
        ),
        (
            "https://api.thegamesdb.net/v1/Publishers",
            Ok(json!({"data": {"publishers": {"4": {"id": 4, "name": "Nintendo"}}}})),
        ),
    ]);
    let source = find("thegamesdb");
    let found = source
        .lookup(
            &api,
            &creds(&[("apiKey", "k")]),
            &query("snes", &snes, None),
        )
        .unwrap()
        .unwrap();
    assert!(
        api.urls()[0].contains("filter%5Bplatform%5D=6"),
        "{:?}",
        api.urls()
    );
    assert!(api.urls()[0].contains("name=Super%20Metroid"));
    let m = &found.metadata;
    assert_eq!(m.genre.as_deref(), Some("Action / Platform"));
    assert_eq!(m.developer.as_deref(), Some("Nintendo R&D1"));
    assert_eq!(m.players.as_deref(), Some("1"));
    assert_eq!(m.age_rating.as_deref(), Some("ESRB:E"));
    assert_eq!(
        found.media,
        [
            (
                MediaKind::Thumbnail,
                "https://cdn/o/boxart/front/7-1.jpg".to_owned()
            ),
            (
                MediaKind::Boxback,
                "https://cdn/o/boxart/back/7-1.jpg".to_owned()
            ),
            (MediaKind::Wheel, "https://cdn/o/clearlogo/7.png".to_owned()),
            // A banner: ES's marquee, and itself.
            (
                MediaKind::Marquee,
                "https://cdn/o/graphical/7-g.jpg".to_owned()
            ),
            (
                MediaKind::Banner,
                "https://cdn/o/graphical/7-g.jpg".to_owned()
            ),
        ]
    );
    // Names are fetched once a run.
    let before = api.urls().len();
    source
        .lookup(
            &api,
            &creds(&[("apiKey", "k")]),
            &query("snes", &snes, None),
        )
        .unwrap();
    assert_eq!(api.urls().len(), before + 2);
    // A platform it does not number: not asked.
    assert!(!source.covers(&query("ports", &[], None)));
}

#[test]
fn igdb_gets_a_token_once_and_posts_a_query() {
    let snes = vec!["snes".to_owned()];
    let api = Api::new(vec![
        (
            "https://id.twitch.tv/oauth2/token",
            Ok(json!({"access_token": "tok", "expires_in": 5000})),
        ),
        (
            "https://api.igdb.com/v4/games",
            Ok(json!([{
                "name": "Super Metroid", "summary": "Samus", "first_release_date": 764035200,
                "genres": [{"name": "Platform"}, {"name": "Adventure"}],
                "involved_companies": [
                    {"company": {"name": "Nintendo R&D1"}, "developer": true, "publisher": false},
                    {"company": {"name": "Nintendo"}, "developer": false, "publisher": true}
                ],
                "total_rating": 93.5,
                "age_ratings": [{"organization": {"name": "PEGI"}, "rating_category": {"rating": "Twelve"}},
                                {"organization": {"name": "CERO"}, "rating_category": {"rating": "CERO_A"}},
                                {"organization": {"name": "ESRB"}, "rating_category": {"rating": "E10"}}], "cover": {"image_id": "co1"},
                "screenshots": [{"image_id": "sc1"}], "artworks": [{"image_id": "ar1"}],
                "game_modes": [{"name": "Single player"}]
            }])),
        ),
    ]);
    let source = find("igdb");
    let c = creds(&[("clientId", "cid"), ("clientSecret", "sec")]);
    let found = source
        .lookup(&api, &c, &query("snes", &snes, None))
        .unwrap()
        .unwrap();
    source
        .lookup(&api, &c, &query("snes", &snes, None))
        .unwrap();
    let sent = api.sent.lock().unwrap();
    assert_eq!(
        sent.iter().filter(|r| r.url.contains("twitch")).count(),
        1,
        "one token"
    );
    let games = sent.iter().find(|r| r.url.contains("igdb")).unwrap();
    assert_eq!(games.method, "POST");
    assert!(games.headers.contains(&("Client-ID".into(), "cid".into())));
    assert!(
        games
            .headers
            .contains(&("Authorization".into(), "Bearer tok".into()))
    );
    let body = String::from_utf8(games.body.clone().unwrap()).unwrap();
    assert!(
        body.starts_with("search \"Super Metroid\";") && body.contains("platforms = (19)"),
        "{body}"
    );
    let m = &found.metadata;
    assert_eq!(m.developer.as_deref(), Some("Nintendo R&D1"));
    assert_eq!(m.publisher.as_deref(), Some("Nintendo"));
    assert_eq!(m.release_date.as_deref(), Some("1994-03-19"));
    assert_eq!(m.players.as_deref(), Some("1"));
    assert_eq!(m.rating, Some(0.935));
    assert_eq!(m.age_rating.as_deref(), Some("ESRB:E10+,PEGI:12,CERO:A"));
    assert_eq!(
        found.media[0],
        (
            MediaKind::Thumbnail,
            "https://images.igdb.com/igdb/image/upload/t_cover_big_2x/co1.jpg".into()
        )
    );
}

#[test]
fn steamgriddb_sends_its_key_and_asks_only_for_what_is_wanted() {
    let api = Api::new(vec![
        (
            "https://www.steamgriddb.com/api/v2/search/autocomplete/",
            Ok(json!({"success": true, "data": [{"id": 42, "name": "Super Metroid"}]})),
        ),
        (
            "https://www.steamgriddb.com/api/v2/grids/game/42",
            Ok(json!({"data": [{"url": "https://sg/grid.png"}]})),
        ),
        (
            "https://www.steamgriddb.com/api/v2/logos/game/42",
            Ok(json!({"data": [{"url": "https://sg/logo.png"}]})),
        ),
    ]);
    let wanted = [MediaKind::Thumbnail, MediaKind::Wheel];
    let q = Query {
        wanted: &wanted,
        ..query("snes", &[], None)
    };
    let found = find("steamgriddb")
        .lookup(&api, &creds(&[("apiKey", "k")]), &q)
        .unwrap()
        .unwrap();
    assert_eq!(
        found.media,
        [
            (MediaKind::Thumbnail, "https://sg/grid.png".to_owned()),
            (MediaKind::Wheel, "https://sg/logo.png".to_owned()),
        ]
    );
    assert!(api.urls().iter().all(|u| !u.contains("heroes")));
    assert!(api.sent.lock().unwrap().iter().all(|r| {
        r.headers
            .contains(&("Authorization".into(), "Bearer k".into()))
    }));
}

#[test]
fn arcadedb_by_short_name_for_arcade_only() {
    let arcade = vec!["arcade".to_owned()];
    let api = Api::new(vec![(
        "https://adb.arcadeitalia.net/service_scraper.php",
        Ok(json!({"release": 6, "result": [{
            "game_name": "sf2", "title": "Street Fighter II: The World Warrior", "manufacturer": "Capcom",
            "url_image_ingame": "https://adb/?mame=sf2&type=ingame", "url_image_marquee": "https://adb/?mame=sf2&type=marquee",
            "url_image_flyer": "https://adb/?mame=sf2&type=flyer", "url_video_shortplay": "https://adb/v.mp4",
            "url_image_cabinet": "https://adb/?mame=sf2&type=cabinet",
            "genre": "Fighter / Versus", "players": 2, "year": "1991",
            "history": "Arcade Video game published 35 years ago:\r\n\r\nStreet Fighter II...\n- TECHNICAL -\nstuff"
        }]})),
    )]);
    let source = find("arcadedb");
    let q = Query {
        stem: "sf2",
        ..query("mame", &arcade, None)
    };
    assert!(source.covers(&q));
    assert!(!source.covers(&query("snes", &["snes".to_owned()], None)));
    let found = source
        .lookup(&api, &Credentials::new(), &q)
        .unwrap()
        .unwrap();
    assert!(api.urls()[0].ends_with("game_name=sf2"));
    assert_eq!(found.metadata.players.as_deref(), Some("2"));
    assert_eq!(found.metadata.release_date.as_deref(), Some("1991"));
    assert_eq!(
        found.metadata.description.as_deref(),
        Some("Street Fighter II...")
    );
    for (kind, url) in [
        (MediaKind::Thumbnail, "https://adb/?mame=sf2&type=flyer"),
        (MediaKind::Flyer, "https://adb/?mame=sf2&type=flyer"),
        (MediaKind::Boxart, "https://adb/?mame=sf2&type=cabinet"),
        (MediaKind::Cabinet, "https://adb/?mame=sf2&type=cabinet"),
    ] {
        assert!(found.media.contains(&(kind, url.into())), "{kind:?}");
    }
    // Not in its database: an empty result.
    let none = Api::new(vec![(
        "https://adb",
        Ok(json!({"release": 6, "result": []})),
    )]);
    assert_eq!(source.lookup(&none, &Credentials::new(), &q).unwrap(), None);
}

#[test]
fn mobygames_parses_html_descriptions_and_basic_genres() {
    let api = Api::new(vec![(
        "https://api.mobygames.com/v1/games",
        Ok(json!({"games": [{
            "title": "Super Metroid", "description": "<p>Samus &amp; the baby.</p>",
            "genres": [{"genre_category": "Basic Genres", "genre_name": "Action"}, {"genre_category": "Perspective", "genre_name": "Side view"}],
            "moby_score": 8.9, "platforms": [{"first_release_date": "1994-04-18"}, {"first_release_date": "1994-03-19"}],
            "sample_cover": {"image": "https://moby/cover.jpg"}, "sample_screenshots": [{"image": "https://moby/s1.jpg"}]
        }]})),
    )]);
    let found = find("mobygames")
        .lookup(&api, &creds(&[("apiKey", "k")]), &query("snes", &[], None))
        .unwrap()
        .unwrap();
    assert_eq!(
        found.metadata.description.as_deref(),
        Some("Samus & the baby.")
    );
    assert_eq!(found.metadata.genre.as_deref(), Some("Action"));
    assert_eq!(found.metadata.release_date.as_deref(), Some("1994-03-19"));
    assert_eq!(found.media.len(), 2);
}

#[test]
fn libretro_guesses_urls_from_the_file_and_game_names() {
    let wanted = [MediaKind::Thumbnail];
    let q = Query {
        stem: "Mario & Luigi (USA)",
        name: "Mario & Luigi",
        wanted: &wanted,
        ..query("snes", &[], None)
    };
    let found = find("libretro")
        .lookup(&Api::new(vec![]), &Credentials::new(), &q)
        .unwrap()
        .unwrap();
    assert_eq!(found.media, [
        (MediaKind::Thumbnail, "https://thumbnails.libretro.com/Nintendo%20-%20Super%20Nintendo%20Entertainment%20System/Named_Boxarts/Mario%20_%20Luigi%20%28USA%29.png".to_owned()),
        (MediaKind::Thumbnail, "https://thumbnails.libretro.com/Nintendo%20-%20Super%20Nintendo%20Entertainment%20System/Named_Boxarts/Mario%20_%20Luigi.png".to_owned()),
    ]);
    assert!(!find("libretro").covers(&query("ports", &[], None)));
}

fn launchbox_zip() -> Vec<u8> {
    use std::io::Write;
    let metadata = r#"<?xml version="1.0"?><LaunchBox>
      <Game><Name>Streets of Rage 2</Name><Platform>Windows</Platform><DatabaseID>9</DatabaseID>
        <CommunityRatingCount>900</CommunityRatingCount></Game>
      <Game><Name>Streets of Rage 2</Name><Platform>Sega Genesis</Platform><DatabaseID>7</DatabaseID>
        <Overview>Mr. X &amp; friends.</Overview><Developer>Sega</Developer><Genres>Action; Beat 'em Up</Genres>
        <MaxPlayers>2</MaxPlayers><ReleaseDate>1992-12-20T00:00:00</ReleaseDate>
        <CommunityRating>4.5</CommunityRating><CommunityRatingCount>10</CommunityRatingCount>
        <ESRB>E10+ - Everyone 10+</ESRB></Game>
      <Game><Name>The Legend of Zelda: A Link to the Past</Name><Platform>Sega Genesis</Platform>
        <DatabaseID>8</DatabaseID><ESRB>Not Rated</ESRB><CommunityRatingCount>0</CommunityRatingCount>
        <CommunityRating>3</CommunityRating></Game>
      <Game><Name>Descent Maximum</Name><Platform>Sega Genesis</Platform><DatabaseID>20</DatabaseID></Game>
      <Game><Name>Descent II</Name><Platform>Sega Genesis</Platform><DatabaseID>21</DatabaseID></Game>
      <Game><Name>Descent</Name><Platform>Sega Genesis</Platform><DatabaseID>22</DatabaseID></Game>
      <GameAlternateName><AlternateName>Descent II</AlternateName><DatabaseID>22</DatabaseID></GameAlternateName>
      <Game><Name>Adventures of Lolo 3</Name><Platform>Sega Genesis</Platform><DatabaseID>23</DatabaseID></Game>
      <Game><Name>Adventures of Lolo 2</Name><Platform>Sega Genesis</Platform><DatabaseID>24</DatabaseID></Game>
      <GameAlternateName><AlternateName>Adventures of Lolo II</AlternateName><DatabaseID>23</DatabaseID><Region>Japan</Region></GameAlternateName>
      <Game><Name>Strikers 1945</Name><Platform>Sega Genesis</Platform><DatabaseID>25</DatabaseID><ReleaseDate>2001-03-27T00:00:00</ReleaseDate></Game>
      <Game><Name>Strikers 1945 II</Name><Platform>Sega Genesis</Platform><DatabaseID>26</DatabaseID><ReleaseDate>1998-10-22T00:00:00</ReleaseDate></Game>
      <GameAlternateName><AlternateName>Strikers 1945 II</AlternateName><DatabaseID>25</DatabaseID><Region>Europe</Region></GameAlternateName>
      <GameAlternateName><AlternateName>Descent 2</AlternateName><DatabaseID>20</DatabaseID><Region>Europe</Region></GameAlternateName>
      <GameAlternateName><AlternateName>Bare Knuckle II</AlternateName><DatabaseID>7</DatabaseID><Region>Japan</Region></GameAlternateName>
      <GameAlternateName><AlternateName>Bare Knuckle II</AlternateName><DatabaseID>7</DatabaseID><Region>Asia</Region></GameAlternateName>
      <GameAlternateName><AlternateName>Streets of Rage 2</AlternateName><DatabaseID>7</DatabaseID></GameAlternateName>
      <GameImage><DatabaseID>7</DatabaseID><FileName>eu.jpg</FileName><Type>Box - Front</Type><Region>Europe</Region></GameImage>
      <GameImage><DatabaseID>7</DatabaseID><FileName>jp.jpg</FileName><Type>Box - Front</Type><Region>Japan</Region></GameImage>
      <GameImage><DatabaseID>7</DatabaseID><FileName>remade.jpg</FileName><Type>Box - Front - Reconstructed</Type><Region>Japan</Region></GameImage>
      <GameImage><DatabaseID>7</DatabaseID><FileName>spine.png</FileName><Type>Box - Spine</Type></GameImage>
      <GameImage><DatabaseID>7</DatabaseID><FileName>skip.png</FileName><Type>Icon</Type></GameImage>
    </LaunchBox>"#;
    let mame = "<LaunchBox><MameFile><FileName>sor2</FileName><Name>Streets of Rage II</Name></MameFile></LaunchBox>";
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, text) in [("Metadata.xml", metadata), ("Mame.xml", mame)] {
        out.start_file(name, options).unwrap();
        out.write_all(text.as_bytes()).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn launchbox_query<'a>(name: &'a str, stem: &'a str, on: &'a [String]) -> Query<'a> {
    Query {
        name,
        stem,
        regions: regions(stem),
        ..query("megadrive", on, None)
    }
}

#[test]
fn launchbox_fetches_its_data_once_and_answers_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("launchbox.db");
    let source = launchbox::LaunchBox::at(file.clone());
    let api = Api {
        answers: vec![("https://gamesdb.launchbox-app.com/", Ok(launchbox_zip()))],
        sent: Mutex::new(Vec::new()),
    };
    let on = vec!["megadrive".to_owned()];
    let no = Credentials::default();

    // A Japanese copy, the profile putting the game's own region first: its Japanese name.
    let q = launchbox_query("Bare Knuckle II", "Bare Knuckle II (Japan)", &on);
    let found = source.lookup(&api, &no, &q).unwrap().unwrap();
    assert_eq!(found.metadata.name.as_deref(), Some("Bare Knuckle II"));
    assert_eq!(
        found.notes.text.get("name").map(String::as_str),
        Some("Japan")
    );
    assert_eq!(
        found.notes.names,
        [(
            "Streets of Rage 2".to_owned(),
            Some("North America".to_owned())
        )]
    );
    // A profile of USA alone: LaunchBox's own name, North America's.
    let usa = ["USA".to_owned()];
    let q = Query {
        regions: &usa,
        ..launchbox_query("Bare Knuckle II", "Bare Knuckle II (Japan)", &on)
    };
    let american = source.lookup(&api, &no, &q).unwrap().unwrap();
    assert_eq!(american.metadata.name.as_deref(), Some("Streets of Rage 2"));
    assert_eq!(
        found.metadata.description.as_deref(),
        Some("Mr. X & friends.")
    );
    assert_eq!(found.metadata.genre.as_deref(), Some("Action, Beat 'em Up"));
    assert_eq!(found.metadata.players.as_deref(), Some("1-2"));
    assert_eq!(found.metadata.release_date.as_deref(), Some("1992-12-20"));
    assert_eq!(found.metadata.rating, Some(0.9));
    assert_eq!(found.metadata.age_rating.as_deref(), Some("ESRB:E10+"));
    // Its other names, each once, not its own again.
    assert_eq!(
        american.notes.names,
        [("Bare Knuckle II".to_owned(), Some("Japan".to_owned()))]
    );
    let base = "https://images.launchbox-app.com/";
    assert_eq!(
        found.media,
        [
            // The ROM's own region first, what was reconstructed after the real scans.
            (MediaKind::Thumbnail, format!("{base}jp.jpg")),
            (MediaKind::Thumbnail, format!("{base}eu.jpg")),
            (MediaKind::Thumbnail, format!("{base}remade.jpg")),
            (MediaKind::Boxside, format!("{base}spine.png")),
        ]
    );

    // A No-Intro name, its "The" in the middle; no rating without votes, none for "Not Rated".
    let q = launchbox_query(
        "Legend of Zelda, The - A Link to the Past",
        "Legend of Zelda, The - A Link to the Past (USA)",
        &on,
    );
    let found = source.lookup(&api, &no, &q).unwrap().unwrap();
    assert_eq!(found.metadata.rating, None);
    assert_eq!(found.metadata.age_rating, None);

    // A MAME short name, through LaunchBox's own table of them.
    let q = launchbox_query("sor2", "sor2", &on);
    assert!(source.lookup(&api, &no, &q).unwrap().is_some());

    // Another game is not guessed at; a platform LaunchBox does not have is not asked.
    let q = launchbox_query("Streets of Rage", "Streets of Rage (USA)", &on);
    assert!(source.lookup(&api, &no, &q).unwrap().is_none());
    let none = vec!["imageviewer".to_owned()];
    assert!(!source.covers(&launchbox_query("x", "x", &none)));

    assert_eq!(api.urls().len(), 1, "the data is fetched once");
    assert!(file.exists());
}

#[test]
fn launchbox_finds_one_game_under_different_names_in_a_library() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::paths::Paths {
        userdata: dir.path().to_path_buf(),
        systems_cfg: std::path::PathBuf::new(),
    };
    let file = paths.cache_dir().join("launchbox.db");
    let mut system = crate::scraper::tests::system(dir.path());
    system.platforms = vec!["megadrive".to_owned()];
    let game = |id, name: &str, path: &str| crate::library::GameFile {
        id,
        name: name.to_owned(),
        path: path.to_owned(),
    };
    let games = [
        game(1, "Bare Knuckle II", "Bare Knuckle II (Japan).md"),
        game(2, "Streets of Rage 2", "Streets of Rage 2 (USA).md"),
        game(3, "Streets of Rage 2", "Streets of Rage 2 (Europe).md"),
        game(
            4,
            "Zelda",
            "Legend of Zelda, The - A Link to the Past (USA).md",
        ),
        game(
            5,
            "Zelda",
            "Legend of Zelda, The - A Link to the Past (Rev 1).md",
        ),
        // LaunchBox's one game twice: `Descent Maximum` (also `Descent 2`) and `Descent II`.
        game(6, "Descent Maximum", "Descent Maximum (USA).md"),
        game(7, "Descent II", "Descent II (Europe).md"),
        // Not taken for one game: a name with no region,
        game(8, "Descent", "Descent (USA).md"),
        // Japan's name for Lolo 3, where the library's Lolo 2 is from the USA,
        game(9, "Adventures of Lolo 2", "Adventures of Lolo 2 (USA).md"),
        game(10, "Adventures of Lolo 3", "Adventures of Lolo 3 (USA).md"),
        // and a name in Europe for a game three years apart.
        game(11, "Strikers 1945", "Strikers 1945 (USA).md"),
        game(12, "Strikers 1945 II", "Strikers 1945 II (Europe).md"),
    ];
    let find = || super::super::duplicates::duplicates(&paths, &system, &games).unwrap();
    assert_eq!(find(), None, "nothing without LaunchBox's data");

    let api = Api {
        answers: vec![("https://gamesdb.launchbox-app.com/", Ok(launchbox_zip()))],
        sent: Mutex::new(Vec::new()),
    };
    let source = launchbox::LaunchBox::at(file);
    let on = vec!["megadrive".to_owned()];
    let q = launchbox_query("x", "x", &on);
    source.lookup(&api, &Credentials::default(), &q).unwrap();

    let groups = find().unwrap();
    // Versions of one title alone are not duplicates: Game versions has them.
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].title, "Descent Maximum");
    let ids: Vec<i64> = groups[0].games.iter().map(|g| g.id).collect();
    assert_eq!(ids, [6, 7]);
    assert_eq!(groups[1].title, "Streets of Rage 2");
    let ids: Vec<i64> = groups[1].games.iter().map(|g| g.id).collect();
    assert_eq!(ids, [1, 2, 3]);
    assert_eq!(find().unwrap(), groups, "asked again: the same answer");

    // A game renamed (its file too) is asked about again; the others are as they were.
    let mut renamed = games.clone();
    renamed[0] = game(1, "Columns", "Columns (Japan).md");
    let groups = super::super::duplicates::duplicates(&paths, &system, &renamed)
        .unwrap()
        .unwrap();
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!(groups[0].title, "Descent Maximum");
}

#[test]
fn launchbox_data_from_an_older_forma_is_built_again() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::paths::Paths {
        userdata: dir.path().to_path_buf(),
        systems_cfg: std::path::PathBuf::new(),
    };
    let file = paths.cache_dir().join("launchbox.db");
    let api = Api {
        answers: vec![("https://gamesdb.launchbox-app.com/", Ok(launchbox_zip()))],
        sent: Mutex::new(Vec::new()),
    };
    assert_eq!(
        super::super::upgrade_launchbox(&api, &paths),
        Ok(false),
        "none yet"
    );
    let source = launchbox::LaunchBox::at(file.clone());
    let on = vec!["megadrive".to_owned()];
    let q = launchbox_query("Bare Knuckle II", "Bare Knuckle II (Japan)", &on);
    source.lookup(&api, &Credentials::default(), &q).unwrap();
    assert_eq!(
        super::super::upgrade_launchbox(&api, &paths),
        Ok(false),
        "up to date"
    );

    // As an older Forma left it: no alternate names as written.
    let db = rusqlite::Connection::open(&file).unwrap();
    db.execute_batch("DROP TABLE alias; PRAGMA user_version = 0;")
        .unwrap();
    drop(db);
    let old = source
        .lookup(&api, &Credentials::default(), &q)
        .unwrap()
        .unwrap();
    assert!(old.notes.names.is_empty(), "it still answers");
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert_eq!(super::super::upgrade_launchbox(&api, &paths), Ok(true));
    let new = source
        .lookup(&api, &Credentials::default(), &q)
        .unwrap()
        .unwrap();
    assert_eq!(new.notes.names.len(), 1, "the source reads the new one");
}

#[test]
fn launchbox_without_its_data_stops_asking_and_keeps_nothing_half_built() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("launchbox.db");
    let source = launchbox::LaunchBox::at(file.clone());
    let api = Api {
        answers: vec![(
            "https://gamesdb.launchbox-app.com/",
            Ok(b"not a zip".to_vec()),
        )],
        sent: Mutex::new(Vec::new()),
    };
    let on = vec!["megadrive".to_owned()];
    let q = launchbox_query("x", "x", &on);
    for _ in 0..3 {
        let e = source.lookup(&api, &Credentials::default(), &q);
        assert!(matches!(e, Err(LookupError::Quota(_))), "{e:?}");
    }
    assert_eq!(api.urls().len(), 1, "not tried for every game");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn screenscraper_says_what_tells_its_media_and_text_apart() {
    let found = ss_parse(&serde_json::json!({
        "noms": [{ "region": "jp", "text": "Sonikku" }],
        "synopsis": [{ "langue": "fr", "text": "Un jeu." }],
        "dates": [{ "region": "ss", "text": "1991-06-23" }],
        "medias": [
            { "type": "video-normalized", "url": "https://s/v", "format": "mp4", "size": "12400000" },
            { "type": "manuel", "url": "https://s/m", "region": "us", "format": "pdf" }
        ]
    }), "Sonic")
    .unwrap();
    let video = &found.notes.media["https://s/v"];
    assert_eq!(video.variant.as_deref(), Some("Small file"));
    assert_eq!(video.format.as_deref(), Some("mp4"));
    assert_eq!(video.size, Some(12_400_000));
    assert_eq!(
        found.notes.media["https://s/m"].region.as_deref(),
        Some("USA")
    );
    assert_eq!(found.notes.text["name"], "Japan");
    assert_eq!(found.notes.text["description"], "French");
    assert!(
        !found.notes.text.contains_key("releaseDate"),
        "ss is no country"
    );
}

#[test]
fn screenscraper_offers_every_name_with_the_regions_using_it() {
    let found = ss_parse(
        &serde_json::json!({
            "noms": [
                { "region": "jp", "text": "Sonikku" },
                { "region": "us", "text": "Sonic" },
                { "region": "eu", "text": "Sonic" },
                { "region": "ss", "text": "Sonic the Hedgehog" }
            ]
        }),
        "Sonic",
    )
    .unwrap();
    assert_eq!(found.metadata.name.as_deref(), Some("Sonic"));
    assert_eq!(
        found.notes.text.get("name").map(String::as_str),
        Some("USA · Europe")
    );
    assert_eq!(
        found.notes.names,
        vec![
            ("Sonic the Hedgehog".to_owned(), None),
            ("Sonikku".to_owned(), Some("Japan".to_owned())),
        ]
    );
}

#[test]
fn screenscraper_puts_the_roms_own_region_first() {
    let game = serde_json::json!({
        "noms": [
            { "region": "us", "text": "Sonic" },
            { "region": "eu", "text": "Sonic the Hedgehog" },
            { "region": "jp", "text": "Sonikku" }
        ],
        "dates": [
            { "region": "us", "text": "1991-06-23" },
            { "region": "eu", "text": "1991-06-21" }
        ],
        "medias": [
            { "type": "box-2D", "url": "https://s/us", "region": "us" },
            { "type": "box-2D", "url": "https://s/eu", "region": "eu" }
        ]
    });
    let europe = ss_parse(&game, "Sonic the Hedgehog (Europe)").unwrap();
    assert_eq!(europe.metadata.name.as_deref(), Some("Sonic the Hedgehog"));
    assert_eq!(europe.notes.text["releaseDate"], "Europe");
    assert_eq!(
        europe.media.first().map(|(_, url)| url.as_str()),
        Some("https://s/eu")
    );
    // A country of Europe: its own, else Europe's.
    let france = ss_parse(&game, "Sonic (France)").unwrap();
    assert_eq!(france.metadata.name.as_deref(), Some("Sonic the Hedgehog"));
    // Japan's own name for a Japanese copy.
    let japan = ss_parse(&game, "Sonic the Hedgehog (Japan)").unwrap();
    assert_eq!(japan.metadata.name.as_deref(), Some("Sonikku"));
    // No region in the name: as before, World, then USA.
    let plain = ss_parse(&game, "Sonic").unwrap();
    assert_eq!(plain.metadata.name.as_deref(), Some("Sonic"));
    assert_eq!(
        plain.media.first().map(|(_, url)| url.as_str()),
        Some("https://s/us")
    );
    assert_eq!(
        super::screenscraper::regions_for(regions("Sonic (USA, Europe) (Rev 1)"))[..3],
        ["us", "eu", "wor"]
    );
}

#[test]
fn hfsdb_signs_in_once_and_reads_a_game() {
    let api = Api::new(vec![
        (
            "https://db.hfsplay.fr/api/v1/auth/token",
            Ok(json!({ "token": "abc" })),
        ),
        (
            "https://db.hfsplay.fr/api/v1/games",
            Ok(json!({ "count": 2, "results": [
                { "name_en": "Super Mario World" },
                {
                    "name_en": "Super Metroid",
                    "description_en": "Samus returns.",
                    "released_at_US": "1994-04-18T00:00:00",
                    "metadata": [
                        { "name": "developer", "value": "Nintendo R&D1" },
                        { "name": "editor", "value": "Nintendo" },
                        { "name": "genre", "value": "Action" },
                        { "name": "players", "value": "2 joueurs" }
                    ],
                    "medias": [
                        { "type": "cover2d", "file": "https://h/back.jpg", "description": "back" },
                        { "type": "cover2d", "file": "https://h/front.jpg", "metadata": [{ "name": "cover2dtype", "value": "front" }] },
                        { "type": "screenshot", "file": "https://h/title.jpg", "description": "title" },
                        { "type": "screenshot", "file": "https://h/play.jpg", "description": "in game" },
                        { "type": "logo", "file": "https://h/logo.png" },
                        { "type": "video", "file": "https://h/v.mp4" },
                        { "type": "manual", "file": "not-a-url" }
                    ]
                }
            ]})),
        ),
    ]);
    let snes = vec!["snes".to_owned()];
    let q = query("snes", &snes, None);
    let source = find("hfsdb");
    let credentials = creds(&[("username", "me"), ("password", "p&ss")]);
    let found = source.lookup(&api, &credentials, &q).unwrap().unwrap();
    assert_eq!(found.metadata.name.as_deref(), Some("Super Metroid"));
    assert_eq!(found.metadata.publisher.as_deref(), Some("Nintendo"));
    assert_eq!(found.metadata.players.as_deref(), Some("2"));
    assert_eq!(found.metadata.release_date.as_deref(), Some("1994-04-18"));
    let urls = |kind| -> Vec<&str> {
        found
            .media
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, u)| u.as_str())
            .collect()
    };
    assert_eq!(urls(MediaKind::Thumbnail), ["https://h/front.jpg"]);
    assert_eq!(urls(MediaKind::Boxback), ["https://h/back.jpg"]);
    assert_eq!(urls(MediaKind::Titleshot), ["https://h/title.jpg"]);
    assert_eq!(urls(MediaKind::Image)[0], "https://h/play.jpg");
    assert_eq!(urls(MediaKind::Video), ["https://h/v.mp4"]);
    assert!(urls(MediaKind::Manual).is_empty());

    source.lookup(&api, &credentials, &q).unwrap();
    let sent = api.sent.lock().unwrap();
    assert_eq!(sent.len(), 3, "one sign-in, two searches");
    assert_eq!(
        sent[0].body.as_deref(),
        Some(&b"username=me&password=p%26ss"[..])
    );
    assert!(
        sent[1]
            .url
            .ends_with("/games?search=Super%20Metroid&system=82351&limit=25")
    );
    assert!(
        sent[1]
            .headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Token abc")
    );
}

#[test]
fn hfsdb_finds_arcade_by_file_and_skips_home_systems() {
    let api = Api::new(vec![
        (
            "https://db.hfsplay.fr/api/v1/auth/token",
            Ok(json!({ "token": "abc" })),
        ),
        (
            "https://db.hfsplay.fr/api/v1/games",
            Ok(json!({ "count": 2, "results": [
                { "name_en": "On the Mega Drive", "system": { "id": 33149 } },
                { "name_en": "Street Fighter II", "system": { "id": 29794 } }
            ]})),
        ),
    ]);
    let arcade = vec!["arcade".to_owned()];
    let q = Query {
        stem: "sf2 (World)",
        name: "sf2",
        ..query("mame", &arcade, None)
    };
    let credentials = creds(&[("username", "me"), ("password", "p")]);
    let found = find("hfsdb")
        .lookup(&api, &credentials, &q)
        .unwrap()
        .unwrap();
    assert_eq!(found.metadata.name.as_deref(), Some("Street Fighter II"));
    assert!(api.urls()[1].ends_with("/games?medias__description=sf2&system_id=29794"));
}

#[test]
fn hfsdb_says_when_the_account_is_refused() {
    let api = Api::new(vec![("https://db.hfsplay.fr/api/v1/auth/token", Err(400))]);
    let snes = vec!["snes".to_owned()];
    let e = find("hfsdb")
        .lookup(
            &api,
            &creds(&[("username", "me"), ("password", "x")]),
            &query("snes", &snes, None),
        )
        .unwrap_err();
    assert!(matches!(e, LookupError::Credentials(_)), "{e:?}");
}

#[test]
fn screenscraper_takes_a_whole_date_over_its_regions_year() {
    let found = ss_parse(
        &serde_json::json!({
            "noms": [{ "region": "us", "text": "Sonic" }],
            "dates": [
                { "region": "us", "text": "1991" },
                { "region": "jp", "text": "1991-07-26" }
            ]
        }),
        "Sonic (USA)",
    )
    .unwrap();
    assert_eq!(found.metadata.release_date.as_deref(), Some("1991-07-26"));
}
