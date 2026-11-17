# Scraper

Forma's scraper finds metadata and media for games and writes them where EmulationStation
does: media into `/userdata/roms/<system>/images|videos|manuals/`, metadata into the system's
`gamelist.xml` (edited in place, as the [write-back](batocera-integration.md) does). Both
frontends see the result. It runs from Forma's menu: Start → ROMs → Media. On a game, Select
offers **Edit details…** and **Scrape…** (see [One game, by hand](#one-game-by-hand)).

## Sources

All built into Forma; add-ons (see [add-ons](addons.md)) can add more. The user turns each on
or off and puts them in order. For each field and each kind of media, the first source in that
order that has it wins.

| Source              | Needs                                              | Matches by                              | Gives                                                                                                                                          |
| ------------------- | -------------------------------------------------- | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| ScreenScraper       | A developer account (see below); the user's login  | Hashes, then file name                  | Everything: boxes 2D/3D/back, screenshots, titles, wheels, marquees, fan art, videos, manuals, maps, cartridges, bezels, metadata              |
| TheGamesDB          | An API key                                         | Name and platform                       | Box front and back, fan art, screenshots, wheels/clear logos, banners, metadata                                                                |
| IGDB                | A Twitch client ID and secret                      | Name and platform                       | Covers, screenshots, artwork, metadata, ratings                                                                                                |
| MobyGames           | An API key                                         | Name and platform                       | Covers, screenshots, metadata                                                                                                                  |
| HFSDB               | API access, from the HFSDB team on Discord         | Name and system; MAME name for arcade   | Boxes front, back and 3D, screenshots, title screens, logos, marquees, fan art, videos, manuals, metadata                                      |
| SteamGridDB         | An API key                                         | Name                                    | Grids (covers), heroes (fan art), logos (wheels)                                                                                               |
| ArcadeDB            | Nothing                                            | MAME short name (arcade only)           | In-game and title shots, marquees, flyers, cabinets, videos, metadata                                                                          |
| LaunchBox           | Nothing (fetches its data file once, about 110 MB) | Name and platform; MAME name for arcade | Boxes front, back, spine and 3D, cartridges and discs, logos, fan art, screenshots, banners, marquees, cabinets, flyers, metadata, age ratings |
| libretro thumbnails | Nothing                                            | No-Intro / Redump name                  | Box fronts, screenshots, title screens, logos                                                                                                  |

ScreenScraper's developer account belongs to Forma, not to each user. CI builds it into the
program from the `SCREENSCRAPER_DEV_ID` and `SCREENSCRAPER_DEV_PASSWORD` repository secrets
(as `FORMA_SCREENSCRAPER_DEV_ID` and `FORMA_SCREENSCRAPER_DEV_PASSWORD`), masked but not
truly hidden. People then enter only their own login, which stays optional. A developer
account entered in the menu replaces the built-in one, and a build without the secrets
requires one there.

Platform IDs (ScreenScraper, TheGamesDB, IGDB) come from EmulationStation's own tables, keyed
by the `<platform>` names in `es_systems.cfg`, with known errors corrected (see
`crates/forma-core/src/scraper/platforms.tsv`). The libretro folder for each Batocera system
is in `libretro.tsv`. MobyGames and SteamGridDB are searched by title alone.

LaunchBox has no API: Forma downloads its `Metadata.zip`, builds `launchbox.db` in its cache
folder (a few hundred MB, once, the first time LaunchBox is asked; again after 30 days), and
matches by name there. Its platforms are in `launchbox.tsv`. Only the pictures are per-game
downloads.

## What the user decides

Start → ROMs → Media has three pages:

- **Accounts**: which sources, in which order, and their credentials. A on a source sets it up;
  Y turns it on or off.
- **Profile**: the regions whose names, boxes, dates and description languages a game gets.
  With _the game's own region first_ (on by default) a ROM's regions in its file name
  (`Sonic (USA, Europe)`) come first, and of several, the one higher in the preferred list
  wins; then the preferred regions, in order (World, USA, Europe to start with). Turned off,
  only the list counts: a French profile gets French boxes and text whatever the ROM says.
  _Download manuals_ (on by default) turns manuals off for every run, even for a theme that
  draws them: they are large. A game's own Scrape on Select still offers its manual, unchosen,
  to add by hand.
- **Scrape**: the systems to scrape (every one to start with), and what to do with games
  already scraped (a description or any media): skip them, fill in what is missing (the
  default), or replace what they have. Then Start; the run goes on in the background, and
  the Media page follows it.

What is fetched is not the user's to choose: it is the theme's (its `scraper` block in
`theme.json`, see theme-authoring.md). Every kind it says it draws (`shows`) is fetched, then
what it asks for on top (`media`, per view too) and its extra fields (`ageRating`). A backup
(`backups`, drawn only when another kind is missing) is saved only when the kind it stands in
for cannot be had. A theme that does not say what it draws gets the usual kinds: box front,
screenshot, marquee, wheel and video. Text details are always fetched.

## One game, by hand

Select on a game offers two things, whatever the theme:

- **Scrape…** asks every scraper that is on and set up, not stopping at the first answer, and
  lists each section (name, description, the box, the video…) with what each scraper has. What
  the game lacks starts on the first scraper's answer; what it already has is kept until the
  user picks otherwise. Pictures show a preview. **Save** writes only those sections.
- **Edit details…** types a field on the on-screen keyboard. Empty clears it, except the name.

A media offer's address never reaches a page: some carry the user's account (ScreenScraper's
do), so the page gets the offer's number, and a small `data:` picture from `scraper_preview`.

## Files

Media are named as EmulationStation names them, e.g. `images/<rom>-thumb.png`,
`images/<rom>-boxback.png`, `videos/<rom>-video.mp4`, `manuals/<rom>-manual.pdf`, and linked
from `gamelist.xml` with the matching tags (`<thumbnail>`, `<boxback>`, `<video>`, `<manual>`…).
Forma's own kinds use their own tags and names the same way: `<boxside>` (`<rom>-boxside.jpg`),
`<boxfull>`, `<banner>`, `<flyer>` and `<cabinet>`. EmulationStation does not read them.

Which source gives which of Forma's own kinds:

| Kind      | ScreenScraper | TheGamesDB | SteamGridDB | ArcadeDB |
| --------- | ------------- | ---------- | ----------- | -------- |
| `boxside` | `box-2D-side` |            |             |          |
| `boxfull` | `box-texture` |            |             |          |
| `banner`  | `steamgrid`   | `banner`   | wide grids  |          |
| `flyer`   | `flyer`       |            |             | flyer    |
| `cabinet` |               |            |             | cabinet  |

ArcadeDB's flyer and cabinet also still fill `thumbnail` and `boxart`, and TheGamesDB's banner
`marquee`, as EmulationStation's themes expect.

## How a run goes

One game at a time, in the background; Forma stays usable and the page shows where it is.
For each game, Forma works out what is still wanted (the chosen kinds it lacks, and details
if any are missing), then asks the turned-on sources that are set up, in order, until nothing
is wanted. Each download is checked to be what it claims (a picture, a video, a PDF) before it
is written, so an error page never becomes a box. A source that refuses its credentials or is
over its quota is not asked again in that run, and the page says so. ROMs up to 256 MB are
hashed for ScreenScraper; bigger ones are matched by name and size.

Credentials are kept in Forma's settings (`scraper.credentials`), like EmulationStation keeps
its own in `es_settings.cfg`. Secret ones are never sent back to a theme, or shown.
