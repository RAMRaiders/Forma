# HTTP fixtures

Responses for the network clients' tests, and the SDK mock's RetroAchievements and netplay data.

**These were written from the documented API formats, not recorded.** The sandbox they were
written in has no outbound network, and RetroAchievements needs a personal API key. They use
the quirks the real APIs are known for (numbers sent as strings, an empty achievement list sent
as `[]` instead of `{}`), but the field set should be checked against live responses:

- `cargo test -p forma-core -- --ignored live_` fetches the real netplay lobby and parses it.
- For RetroAchievements, replace `ra-user.json` / `ra-game.json` with real responses for any
  account, scrubbed of anything personal.

| File           | Endpoint                                                              |
| -------------- | --------------------------------------------------------------------- |
| `ra-user.json` | `retroachievements.org/API/API_GetUserSummary.php`                    |
| `ra-game.json` | `retroachievements.org/API/API_GetGameInfoAndUserProgress.php` (1446) |
| `lobby.json`   | `lobby.libretro.com/list/`                                            |

IP addresses in `lobby.json` are from the documentation ranges (192.0.2.0/24, 198.51.100.0/24).

- `libretro-boxart.png`: a 2×3 picture, served as the libretro thumbnails box for Super Mario
  Bros. (World), for the scraper.
