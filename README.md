# GameTracker

A desktop tracker for the games you own, the ones you want, and what they cost right now.
Tauri 2 + Vue 3, local-first SQLite, metadata from IGDB, prices from IsThereAnyDeal,
CheapShark and the PlayStation Store.

Personal, non-commercial project. See [docs/PLAN.md](docs/PLAN.md) for the full design.

## Requirements

- **Node 22** — an `.nvmrc` is included, so `nvm use` picks it up.
- **Rust stable** (1.98 or newer works; Tauri's floor is 1.77.2).
- Platform toolchain: Xcode Command Line Tools on macOS, MSVC build tools + WebView2 on Windows.

## Getting started

```bash
nvm use
npm install
npm run app
```

The first launch creates a `.env` in the project root (development) or in the app's config
directory (installed builds), and a SQLite database at:

- macOS — `~/Library/Application Support/com.niklasheneka.gametracker/gametracker.db`
- Windows — `%APPDATA%\com.niklasheneka.gametracker\gametracker.db`

A development build (`npm run app`) uses `gametracker-dev.db` in the same folder instead, so
nothing done while developing ever shows up in the installed app.

## Credentials

All keys live in `.env`, which is git-ignored and created with owner-only permissions.
**They are read by the Rust backend only** — nothing from `.env` is bundled into the
frontend or sent across the IPC boundary, so the interface can never leak them. The
Settings screen shows which keys are loaded (as `abcd…wxyz` hints) and can reopen the file
or reload it without restarting.

| Variable | Needed for | Where to get it |
|---|---|---|
| `IGDB_CLIENT_ID` / `IGDB_CLIENT_SECRET` | **Required.** Covers, genres, platforms, search | [dev.twitch.tv/console/apps](https://dev.twitch.tv/console/apps) — redirect URL `http://localhost` |
| `ITAD_API_KEY` | Better PC prices, sale expiry, price history | [isthereanydeal.com/apps/my](https://isthereanydeal.com/apps/my/) |
| `STEAM_API_KEY` | Library import with playtime | [steamcommunity.com/dev/apikey](https://steamcommunity.com/dev/apikey) |

Only the IGDB pair is required. Without an ITAD key, prices fall back to CheapShark, which
needs no key at all.

A one-off override without editing the file:

```bash
IGDB_CLIENT_ID=xxx IGDB_CLIENT_SECRET=yyy npm run app
```

## Play Next

A single ordered list of what to play next, separate from the board's four columns. Add
anything already in your library — owned or not, including a finished game worth another run
after a big update — and drag the rows by the grip on the left to set the order. The two lists
keep their own ordering, so arranging one never disturbs the other, and removing a game from
your library removes it here too.

New rows join the **end**: the top slot means "this is the one", and it should not be taken by
whatever you added last. A game you own shows what you own it on and how long you have played
instead of a price, since that question is already settled.

Each row also carries the facts that actually decide what to play next — roughly how long the
game takes, its IGDB rating and Steam's own verdict ("Very Positive") — plus your own note from
the detail drawer, if you wrote one. Playtimes come from IGDB's `game_time_to_beats` endpoint
and are only shown when enough players agree and their three figures are consistent; a game
with four contradictory submissions says nothing rather than printing a number the app cannot
stand behind. Steam verdicts come from the public storefront, need no key, and are re-checked
monthly.

For anything you do not own yet, the row shows **one store at a time** — its price with any
discount, and when that store's next storewide sale starts along with what the game cost during
the last one. Use the store button to switch; the choice is remembered per game, and stores
that do not sell it stay in the list because their next sale is still an answer to "should I
wait?". Prices are read from the local cache, never fetched on opening the page; **Refresh
prices** turns amber when something is older than six hours and checks just the unowned games
on this list rather than your whole wishlist.

## Importing from Steam

Settings → Steam library. Enter your SteamID64, your profile name, or paste your profile URL.
Your Steam profile **and game details** must be set to Public, or Steam returns an empty list.

The preview shows how many games you own, how many you have played and how many you have never
launched, so you can import just the backlog if you prefer. Importing is additive and safe to
repeat: it never re-categorises a game you have already sorted and never overwrites playtime
you typed yourself.

## PlayStation prices

PlayStation prices come from the PS Store's own GraphQL endpoint. It needs no account, but it
accepts **only persisted queries whose hash Sony has whitelisted**. Sony changes a hash when it
changes the query behind it — rarely: the one the app uses was published in October 2023 and is
still accepted. The hash lives in `src-tauri/ps-store-queries.json` rather than in the binary.

**If Sony refuses the hash, PlayStation prices keep working.** The app reads the same price
from the store page (`store.playstation.com/<locale>/concept/<id>`), which needs no hash, and
Settings shows a notice with an amber dot on its sidebar entry. The page is a much bigger
download (~700 KB against a few hundred bytes), which is why it is the fallback rather than the
first choice. It is also the richer source: it carries sale end dates the query often omits.

To restore the lighter request, you need a current hash — and **it cannot be copied from your
browser**: store.playstation.com never sends this query (its pages are rendered on Sony's
servers; the operation belongs to Sony's other clients, most likely the PlayStation App).
Open-source PlayStation Store libraries publish these hashes, so search GitHub code for
`metGetPricingDataByConceptId` and take the most recently updated value. Then copy
`ps-store-queries.json` next to your `.env`, replace `sha256Hash`, and click **Reload keys** in
Settings — no restart needed. The notice clears itself after the next successful refresh.

Prices that only PlayStation Plus members get are ignored on both paths. A game in the Plus
catalogue has a member price of €0.00, and treating that as a purchase price made such games
look "100% off" to everyone; a game you can only get through Plus now simply has no
PlayStation price.

Any store can also be priced by hand from the game's price panel — which is how **Nintendo
and Xbox** prices get in. Neither has an automatic source: Xbox has no usable free API, and
Nintendo's price endpoint works but IGDB has no eShop id to match a game to, so every lookup
would hang on a fuzzy title search. Manual prices are never overwritten by an automatic
refresh.

## Sale calendar

`src-tauri/sale-calendar.json` lists known storewide sale events (Steam Autumn Sale, Days of
Play, Epic MEGA Sale …). The deals panel uses it to show the next sale per shop and what a
game cost during the previous run of the same event.

Dates marked `"confirmed": true` are published by the store; the rest are estimated from
previous years and labelled as such in the app. To correct or extend them, copy the file to
the same directory as your `.env` — that copy wins, so your edits survive updates:

```bash
cp src-tauri/sale-calendar.json .
```

## Tests

```bash
cd src-tauri && cargo test --lib
```

Live tests hit the real IGDB API and are skipped by default. IGDB **silently ignores field
names it does not recognise** rather than erroring, so a renamed field looks like
permanently-missing data; these tests assert the fields still arrive. Run them when
something metadata-shaped breaks:

```bash
cd src-tauri && cargo test --lib live -- --ignored --nocapture
```

## Building installers

**Released versions** are on the [Releases page](https://github.com/NiklasHeneka/GameTracker/releases):
pushing an annotated `v*` tag builds a universal macOS `.dmg` plus a Windows `.msi` and NSIS
`-setup.exe`, and publishes them with the tag's message as the release notes. The tag must be
annotated (`git tag -a`) — the release job refuses a lightweight one rather than publish a
commit subject as the title.

To build locally:

```bash
npm run app:build
```

Produces `src-tauri/target/release/bundle/` — a `.dmg` and `.app` on macOS, `.msi` and an
NSIS `.exe` on Windows. A universal macOS binary needs
`npm run tauri -- build --target universal-apple-darwin` and both Rust targets installed.

**Installed builds keep credentials somewhere else.** The dev server reads the `.env` beside
this README; a packaged app reads one in its own config directory and creates a blank
template there on first launch:

- macOS — `~/Library/Application Support/com.niklasheneka.gametracker/.env`
- Windows — `%APPDATA%\com.niklasheneka.gametracker\.env`

Settings → **Show .env** opens exactly that file, wherever the running build looks for it.
Until it has your IGDB keys, the sidebar shows a *setup* badge.

Builds are **unsigned** — signing needs a paid Apple Developer certificate and a Windows
code-signing certificate. macOS will refuse an unsigned app on first launch; right-click the
app and choose Open, or run
`xattr -dr com.apple.quarantine /Applications/GameTracker.app`.

## Scripts

| Command | What it does |
|---|---|
| `npm run app` | Run the desktop app in development |
| `npm run app:build` | Build installers (`.dmg`/`.app`, `.msi`/NSIS) |
| `npm run dev` | Vite dev server alone — no IPC, so most of the app will not work |
| `npm run build` | Typecheck + build the frontend bundle |

## Project layout

```
src/          Vue frontend — views, components, Pinia stores, typed IPC wrappers
src-tauri/    Rust backend — SQLite, migrations, credential loading, API clients
docs/PLAN.md  Design and build phases
```

Every outbound HTTP request is made from Rust, never the webview. That sidesteps CORS
(IGDB sends no CORS headers), keeps secrets out of the frontend, and puts rate limiting and
caching in one place. The webview's CSP allows no network access beyond IPC and the cover
image CDNs.

## Attribution

Game metadata and playtimes by [IGDB](https://www.igdb.com/). Review scores by
[Steam](https://store.steampowered.com/). Prices by
[IsThereAnyDeal](https://isthereanydeal.com/) and [CheapShark](https://www.cheapshark.com/).
PlayStation prices from the [PlayStation Store](https://store.playstation.com/).
