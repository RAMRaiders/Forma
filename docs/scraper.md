# Scraper

Forma's scraper finds metadata and media for games and writes them where EmulationStation
does: media into `/userdata/roms/<system>/images|videos|manuals/`, metadata into the system's
`gamelist.xml` (edited in place, as the [write-back](batocera-integration.md) does). Both
frontends see the result. It runs from Forma's menu: Start → Games → Scrapers. On a game, Select
offers **Edit details…** and **Scrape…** (see [One game, by hand](#one-game-by-hand)).

## Sources

All built into Forma; add-ons (see [add-ons](addons.md)) can add more. The user turns each on
or off and puts them in order. For each field and each kind of media, the first source in that
order that has it wins.

| Source              | Needs                                              | Matches by                              | Gives                                                                                                                                          |
| ------------------- | -------------------------------------------------- | --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| ScreenScraper       | A developer ID and password, and the user's login  | Hashes, then file name                  | Everything: boxes 2D/3D/back, screenshots, titles, wheels, marquees, fan art, videos, manuals, maps, cartridges, bezels, metadata              |
| TheGamesDB          | An API key                                         | Name and platform                       | Box front and back, fan art, screenshots, wheels/clear logos, banners, metadata                                                                |
| IGDB                | A Twitch client ID and secret                      | Name and platform                       | Covers, screenshots, artwork, metadata, ratings                                                                                                |
| MobyGames           | An API key                                         | Name and platform                       | Covers, screenshots, metadata                                                                                                                  |
| SteamGridDB         | An API key                                         | Name                                    | Grids (covers), heroes (fan art), logos (wheels)                                                                                               |
| ArcadeDB            | Nothing                                            | MAME short name (arcade only)           | In-game and title shots, marquees, flyers, cabinets, videos, metadata                                                                          |
| LaunchBox           | Nothing (fetches its data file once, about 110 MB) | Name and platform; MAME name for arcade | Boxes front, back, spine and 3D, cartridges and discs, logos, fan art, screenshots, banners, marquees, cabinets, flyers, metadata, age ratings |
| libretro thumbnails | Nothing                                            | No-Intro / Redump name                  | Box fronts, screenshots, title screens, logos                                                                                                  |

Platform IDs (ScreenScraper, TheGamesDB, IGDB) come from EmulationStation's own tables, keyed
by the `<platform>` names in `es_systems.cfg`, with known errors corrected (see
`crates/forma-core/src/scraper/platforms.tsv`). The libretro folder for each Batocera system
is in `libretro.tsv`. MobyGames and SteamGridDB are searched by title alone.

LaunchBox has no API: Forma downloads its `Metadata.zip`, builds `launchbox.db` in its cache
folder (a few hundred MB, once, the first time LaunchBox is asked; again after 30 days), and
matches by name there. Its platforms are in `launchbox.tsv`. Only the pictures are per-game
downloads.

## What the user decides

- Which sources, in which order, and their credentials.
- Which media kinds to fetch: any of image (screenshot), thumbnail (box front), boxback,
  box 3D, marquee, wheel (logo), fan art, title screen, cartridge, map, bezel, video, manual,
  mix; and Forma's own: box spine, full box cover, banner, flyer and cabinet (arcade).
- Whether to fetch what the theme in use asks for too (its `scraper` block in `theme.json`:
  more media kinds, and age ratings); on by default. See theme-authoring.md.
- Missing only (the default) or replace what is there.
- What to scrape: the selected game, a system, or every system.

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
