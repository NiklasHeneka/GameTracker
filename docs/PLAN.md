# GameTracker — Implementation Plan

A desktop backlog/wishlist tracker for video games. Tauri 2 (Rust) + Vue 3 frontend,
local-first SQLite, metadata from IGDB, price/deal data from IsThereAnyDeal + CheapShark
(+ optional console sources). macOS and Windows.

Status: **plan — nothing implemented yet.** Working dir is empty.

---

## 1. What the app does

Two axes, not one list. This gives the user's two requested buckets *and* more, for free:

| | **Want to play** | **Playing** | **Finished / Dropped** |
|---|---|---|---|
| **Owned** | Backlog | Playing | Finished |
| **Not owned** | Wishlist | — | — |

- **Wishlist** = want to play, don't own → this is where price/deal tracking matters most.
- **Backlog** = own it, haven't played it → the "todo list" core.
- A game moves Wishlist → Backlog when marked *bought* (records purchase price + date + store,
  which later powers the "money saved by waiting" stat).
- Backlog → Playing → Finished, with hours played, personal rating, notes.

Each game shows: cover art, genres, platforms, release date, summary, and a **deals panel**
with current price per store, discount %, when the current sale ends, all-time low,
when it was last on sale, and an *estimated* next sale window.

---

## 2. Tech stack

| Layer | Choice | Why |
|---|---|---|
| Shell | **Tauri 2** | Native macOS + Windows, small binaries, Rust backend |
| Frontend | **Vue 3** (`<script setup>`) + **TypeScript** + **Vite** | Requested |
| State | **Pinia** | Simple stores per domain (library, deals, settings) |
| Routing | **Vue Router** (memory history) | 5 views |
| Styling | **Tailwind CSS v4** (Vite plugin) + custom CSS vars | Fast path to a dense, dark, cover-art-heavy UI |
| Motion | **@vueuse/motion** or plain CSS transitions | Card hovers, drawer slide-in |
| Drag & drop | **vuedraggable** (SortableJS) | Move cards between status columns |
| Charts | **uPlot** or hand-rolled inline SVG sparkline | Price history; both tiny |
| DB | **SQLite** via `rusqlite` (bundled) | Price history needs real queries; no server |
| HTTP | **reqwest** (Rust side only) | See §3 |
| Secrets | **`.env`** read by Rust via `dotenvy` (owner-only permissions) | Chosen over the OS keychain: one file the user can edit directly. Trade-off noted in §9. |
| Scheduling | **tokio** interval task in Rust | Background price refresh |
| Notifications | `tauri-plugin-notification` | Price-drop alerts |
| Misc plugins | `tauri-plugin-opener` (open store pages), `tauri-plugin-dialog` (import/export) | |

### All network calls go through Rust, never the webview

This is the single most important architectural decision:

1. **CORS.** IGDB (`api.igdb.com`) sends no CORS headers — a `fetch()` from the webview fails.
   `reqwest` from Rust has no such restriction.
2. **Secrets.** The Twitch client secret and ITAD key stay in the Rust process + OS keychain,
   never in JS memory or devtools.
3. **One choke point** for rate limiting (IGDB 4 req/s, ITAD 1000 req/5min, PlatPrices
   1000 req/**month**), retry/backoff, and the response cache.

Frontend talks only to `#[tauri::command]` functions. It never sees a URL or a key.

---

## 3. External APIs — verified 2026-08-20

### IGDB (metadata, covers, genres, platforms) — required
- Auth: Twitch OAuth2 **client credentials**. `POST https://id.twitch.tv/oauth2/token`
  with `client_id`, `client_secret`, `grant_type=client_credentials` → app access token
  (valid ~60 days). Send `Client-ID` + `Authorization: Bearer <token>` on every call.
  Cache the token in the DB; refresh on 401.
- Base: `https://api.igdb.com/v4/{endpoint}`, POST with an **Apicalypse** body, e.g.
  `fields name,cover.image_id,genres.name,platforms.abbreviation,first_release_date,summary,total_rating; search "hollow knight"; limit 20;`
- Endpoints used: `games`, `covers`, `artworks`, `screenshots`, `genres`, `platforms`,
  `release_dates`, `involved_companies`, `external_games` (→ Steam appid!), `multiquery`.
- `external_games` with `category = 1` gives the **Steam appid**, which is the join key to
  every price API. Fetch it with the game.
- Images: `https://images.igdb.com/igdb/image/upload/t_{size}/{image_id}.jpg`
  sizes: `cover_small` 90×128, `cover_big` 227×320, `screenshot_med` 569×320,
  `screenshot_big` 889×500, `screenshot_huge`/`720p` 1280×720, `1080p` 1920×1080,
  `thumb` 90×90, `logo_med`, `micro`. Append `_2x` to the size for retina; `.webp` also served.
- Rate limit: **4 requests/second**, 8 open requests max → token-bucket in Rust.
- Terms: free for non-commercial use. ✅ fits this project.
- **Setup the user must do once:** register an app at dev.twitch.tv → Client ID + Secret.

### IsThereAnyDeal (PC prices, sale history, sale expiry) — required
- Auth: free API key from isthereanydeal.com app registration.
  Send as `ITAD-API-Key` header (or `?key=`). Confirmed: **403 without a key**.
- Rate limit: 1000 requests / 5 minutes with a verified email. Generous.
- Endpoints:
  - `GET /games/lookup/v1?title=` or `?appid=<steam appid>` → ITAD game UUID. **Steam appid is the
    reliable join** — title matching is fuzzy and wrong for remasters/editions.
  - `POST /games/prices/v3` (body: up to 200 UUIDs; `country`, `shops`, `deals`) → per-shop
    `price`, `regular`, `cut`, `url`, `drm`, `timestamp`, **`expiry`** ← *"sale ends in 3d"*.
  - `POST /games/historylow/v1` → all-time low, 3-month low, 1-year low.
  - `POST /games/storelow/v2` → historical low per shop.
  - `GET /games/history/v2?id=&since=` → price change log ← *"last on sale: €7.49 on 2026-06-28"*.
  - `GET /deals/v2` → storewide deals feed (used for the Deals view).
  - `GET /games/overview/v2` → current best + historical low + active bundles in one call.
- Terms: non-commercial fine; **must link to IsThereAnyDeal.com or credit the ITAD API**;
  must not strip affiliate tags from returned URLs. → put "Prices by IsThereAnyDeal" in the
  deals panel footer and open store links exactly as returned.
- Scope: **PC only** (Steam, GOG, Epic, Humble, Fanatical, GMG, …).

### CheapShark (PC prices — fallback / no-key path)
- No key, no auth. Base `https://www.cheapshark.com/api/1.0`.
- ⚠️ **Verified today: it now rejects requests with a missing or generic User-Agent**
  (`{"error": "Missing or generic User-Agent header detected..."}`). Must send e.g.
  `GameTracker/0.1 (you@example.com)`.
- `GET /games?title=&limit=` → `gameID`, `steamAppID`, `cheapest`, `thumb`.
- `GET /games?id=` → `info`, **`cheapestPriceEver` {price, date}**, `deals[]` per store.
- `GET /stores` → 35 stores, `isActive` flag (Steam/GOG/Humble/GMG/Fanatical/Epic active).
- `GET /deals?...` → filterable deals feed.
- Value: works **before** the user has configured any key → the app is useful on first launch.
  Ship it as the default PC price source, and let ITAD upgrade it once a key is entered.

### Console prices — PlayStation first-class, Switch nice-to-have
| Platform | Priority | Source | Reality |
|---|---|---|---|
| PlayStation | **first-class** (Phase 2b) — **PS Store GraphQL**, with manual entry as fallback | `https://web.np.playstation.com/api/graphql/v1/op` (PlatPrices is out: keys go only to "qualifying projects") | **Verified reachable 2026-09-07.** Free, no account. Details that matter, all confirmed by probing it: it accepts **only persisted queries whose sha256Hash Sony has whitelisted** — an unknown hash returns `Query <hash> not whitelisted`, and arbitrary GraphQL is refused outright. It also enforces an Apollo CSRF guard: requests need `x-apollo-operation-name` **and** `apollo-require-preflight: true`, or they are rejected before reaching the resolver. Region comes from `x-psn-store-locale-override` (e.g. `de-DE`). With a valid published hash the endpoint reaches real business logic (a bad category id returns a normal `data_not_found`), so it works. |
| Nintendo Switch | nice-to-have (Phase 3, feature-flagged) | `https://api.ec.nintendo.com/v1/price?country=DE&lang=en&ids=<nsuid,…>` | Verified reachable today; returns `regular_price` + `discount_price` with `start_datetime`/`end_datetime` — a real sale-end date, better data than PlatPrices gives. Undocumented but stable and widely used. The friction is resolving an `nsuid` from a title (eShop Algolia search). Ships behind a toggle, off by default; if it breaks it takes out one row of the deals panel and nothing else. |
| Xbox | **out of scope** | — | No usable free API; not a platform you buy on. Xbox appears only as an IGDB platform chip on the game's metadata. Dropped deliberately rather than half-built. |

### Steam Web API — confirmed in scope
- `GetOwnedGames` (free key from steamcommunity.com/dev/apikey, profile must be public)
  → auto-import owned games **with playtime hours**. Turns setup from "add 200 games by hand"
  into one button. Returns `appid` + `playtime_forever`, which resolves to IGDB via
  `external_games` — the same Steam-appid join used everywhere else.
- Import rules: create entries as `owned = 1`; `status = playing` if played in the last 2 weeks
  (`playtime_2weeks`), `finished` never guessed, otherwise `want` (backlog). Never overwrite an
  entry the user has already edited — import is additive, and re-running it is safe.
- `GetPlayerAchievements` could later drive a completion %.

---

## 4. "When is it next on sale?" — being honest

> **Revised twice.** The cadence estimate below did not survive contact with real data and
> has been removed; the *storewide sale calendar* has since been built instead and is the
> honest answer to "when is it next on sale". With ~20 shops running staggered promotions, a popular game is
> discounted *somewhere* on most days: Disco Elysium was discounted on 401 of 601 observed
> days, and the detector duly reported "145 sales, one every 10 days". No threshold rescues
> this, because "how often does it go on sale" is not a well-defined question for a catalogue
> of independent sellers. The panel now reports what the data does support — **what share of
> the observed window had any discount, and how deep the best one went** ("discounted on 66%
> of the last 601 days; the deepest was −100%"). That answers "should I wait?" honestly.
> The genuinely predictive half of the original idea — the bundled calendar of announced
> storewide sale events — **is now built** (see below).

### Storewide sale calendar — the actual "next sale" answer

`src-tauri/sale-calendar.json` holds dated sale events per shop, and the deals panel shows,
for every shop the user buys from: the next sale, when it starts, whether the dates are
**confirmed by the store** or estimated from previous years, and — the part that makes it
useful — **what this game cost during the previous run of that same sale**, read out of the
stored price history for that shop within that window.

So Disco Elysium reads: *"Steam · Autumn Sale · in 22 days · 1.–8. Okt. 2026 · Not discounted
here in the last Autumn Sale."* That answers "should I wait for the Steam sale?" with evidence
rather than a guess.

Design notes:
- Dates are **data, not code**. Copy the file next to `.env` and the app prefers that copy, so
  corrections survive updates and Valve's yearly reshuffles.
- `confirmed: true` only where the store has published dates. Steam publishes its calendar a
  year ahead; the rest are patterns from previous years and are labelled *estimated*.
- Past events are as important as future ones — they are the lookup window for "last time".
- "Not discounted" and "no price recorded then" are **different answers** and are worded
  differently; conflating them would be a quiet lie.
- Timestamps crossing to the frontend are **midday UTC**, not midnight. An 8 October end date
  stored as end-of-day UTC rendered as 9 October in Berlin.


No API can tell you when a game *will* be discounted. Nothing published predicts it, and
anything claiming to is guessing. So the app shows three real things and one clearly-labelled
estimate:

1. **Sale ends** — from ITAD's `expiry` on the current deal. Real data. `Sale ends in 3d 4h`.
2. **Last on sale** — from ITAD `/games/history/v2` + our own accumulating snapshots.
   `Last sale: −50% (€7.49), ended 28 Jun 2026`.
3. **All-time / 1-year low** — from `historylow` + CheapShark `cheapestPriceEver`.
   `All-time low €2.99 — current price is 5× that` is the actually useful buy/wait signal.
4. **Estimated next sale** *(labelled "estimate")* — two inputs:
   - a small bundled, user-editable `sale-calendar.json` of known storewide events
     (Steam Spring/Summer/Autumn/Winter, GOG sales, PSN Days of Play…) →
     `Steam Autumn Sale starts in 41 days`;
   - a cadence heuristic from price history → `usually discounted every ~90 days;
     74 days since the last one`.

   Rendered as a hint with an "estimate" chip, never as a promise.

---

## 5. Data model (SQLite)

```sql
-- metadata cache, keyed by IGDB
CREATE TABLE game (
  id                INTEGER PRIMARY KEY,        -- IGDB id
  name              TEXT NOT NULL,
  slug              TEXT,
  summary           TEXT,
  cover_image_id    TEXT,                       -- images.igdb.com id
  artwork_image_id  TEXT,
  first_release     INTEGER,                    -- unix
  igdb_rating       REAL,
  steam_appid       INTEGER,                    -- join key for prices
  itad_uuid         TEXT,
  cheapshark_id     TEXT,
  metadata_fetched  INTEGER NOT NULL
);
CREATE TABLE genre    (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE platform (id INTEGER PRIMARY KEY, name TEXT NOT NULL, abbreviation TEXT, family TEXT);
CREATE TABLE game_genre    (game_id INTEGER, genre_id INTEGER,    PRIMARY KEY (game_id, genre_id));
CREATE TABLE game_platform (game_id INTEGER, platform_id INTEGER, PRIMARY KEY (game_id, platform_id));

-- the user's actual list
CREATE TABLE entry (
  id             INTEGER PRIMARY KEY AUTOINCREMENT,
  game_id        INTEGER NOT NULL REFERENCES game(id),
  owned          INTEGER NOT NULL DEFAULT 0,        -- 0/1
  status         TEXT NOT NULL,                     -- want | playing | finished | dropped
  priority       INTEGER NOT NULL DEFAULT 0,        -- manual sort within a column
  own_platform   TEXT,                              -- 'Steam' | 'PS5' | 'Switch' | ...
  target_price   REAL,                              -- alert threshold
  price_at_add   REAL,                              -- for the "saved by waiting" stat
  purchase_price REAL,
  purchase_date  INTEGER,
  purchase_store TEXT,
  hours_played   REAL,
  my_rating      INTEGER,                           -- 1..10
  notes          TEXT,
  added_at       INTEGER NOT NULL,
  started_at     INTEGER,
  finished_at    INTEGER,
  UNIQUE (game_id)
);

-- price data
CREATE TABLE price_snapshot (               -- current best price per shop, upserted
  game_id INTEGER NOT NULL, shop TEXT NOT NULL, platform_family TEXT NOT NULL, -- pc|playstation|nintendo
  country TEXT NOT NULL, currency TEXT NOT NULL,
  price REAL NOT NULL, regular REAL, cut INTEGER, url TEXT,
  sale_expiry INTEGER, fetched_at INTEGER NOT NULL, source TEXT NOT NULL,
  PRIMARY KEY (game_id, shop, country)
);
CREATE TABLE price_low (                    -- all-time / 1y / 3m lows
  game_id INTEGER NOT NULL, shop TEXT NOT NULL, country TEXT NOT NULL, scope TEXT NOT NULL,
  price REAL NOT NULL, cut INTEGER, occurred_at INTEGER, fetched_at INTEGER NOT NULL,
  PRIMARY KEY (game_id, shop, country, scope)
);
CREATE TABLE price_history (                -- change log, for the sparkline + "last on sale"
  game_id INTEGER NOT NULL, shop TEXT NOT NULL, country TEXT NOT NULL,
  ts INTEGER NOT NULL, price REAL NOT NULL, regular REAL, cut INTEGER,
  PRIMARY KEY (game_id, shop, country, ts)
);
CREATE TABLE alert_log (game_id INTEGER, shop TEXT, price REAL, notified_at INTEGER);

-- infrastructure
CREATE TABLE api_cache (key TEXT PRIMARY KEY, body TEXT NOT NULL, fetched_at INTEGER, ttl INTEGER);
CREATE TABLE setting   (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE migration (version INTEGER PRIMARY KEY, applied_at INTEGER);
```

DB location: `app_data_dir()/gametracker.db`. Covers cached to `app_cache_dir()/covers/`.

---

## 6. Project layout

```
GameTracker/
├─ docs/PLAN.md
├─ package.json  vite.config.ts  tsconfig.json  index.html
├─ src/                                   # Vue
│  ├─ main.ts  App.vue  router.ts
│  ├─ api/                                # thin typed wrappers over invoke()
│  │  ├─ ipc.ts        games.ts  library.ts  prices.ts  settings.ts
│  ├─ stores/          library.ts  deals.ts  settings.ts  ui.ts
│  ├─ types/           models.ts           # mirrors the Rust structs
│  ├─ composables/     useCover.ts  useCountdown.ts  useCommandPalette.ts
│  ├─ components/
│  │  ├─ GameCard.vue           # cover, hover overlay, status ring, deal badge
│  │  ├─ GameGrid.vue  StatusColumn.vue    # draggable
│  │  ├─ GameDetailDrawer.vue   # hero artwork + info + deals
│  │  ├─ DealsPanel.vue  PriceRow.vue  PriceSparkline.vue  SaleCountdown.vue
│  │  ├─ AddGameModal.vue       # IGDB search-as-you-type
│  │  ├─ GenreChip.vue  PlatformChip.vue  StarRating.vue
│  │  └─ CommandPalette.vue  EmptyState.vue  SkeletonCard.vue
│  ├─ views/  LibraryView.vue  DealsView.vue  StatsView.vue  SettingsView.vue
│  └─ styles/  main.css  tokens.css
└─ src-tauri/
   ├─ Cargo.toml  tauri.conf.json  build.rs  icons/
   └─ src/
      ├─ main.rs  lib.rs
      ├─ db/         mod.rs  migrations.rs  models.rs  queries.rs
      ├─ commands/   library.rs  search.rs  prices.rs  settings.rs  stats.rs  import.rs
      ├─ clients/    igdb.rs  itad.rs  cheapshark.rs  platprices.rs  nintendo.rs  steam.rs
      ├─ services/   metadata.rs  pricing.rs  sale_estimate.rs  refresh.rs  notify.rs
      ├─ util/       rate_limit.rs  cache.rs  keyring.rs  error.rs
      └─ sale_calendar.json
```

### Tauri command surface (the whole frontend API)

```rust
// search & metadata
search_games(query: String)                  -> Vec<GameSearchResult>
get_game(igdb_id: i64)                       -> GameDetail          // cached, refetch if stale
// library
list_entries(filter: EntryFilter)            -> Vec<LibraryEntry>
add_entry(igdb_id: i64, owned: bool, platform: Option<String>) -> LibraryEntry
update_entry(id: i64, patch: EntryPatch)     -> LibraryEntry        // status/owned/rating/notes/hours
mark_purchased(id: i64, price: f64, store: String, platform: String) -> LibraryEntry
reorder_entries(ids: Vec<i64>)               -> ()
delete_entry(id: i64)                        -> ()
// prices
get_prices(igdb_id: i64, force: bool)        -> PriceOverview       // shops + lows + expiry + estimate
refresh_wishlist_prices()                    -> RefreshReport       // also runs on a timer
list_active_deals()                          -> Vec<DealRow>        // Deals view
// misc
get_settings() / set_settings(..)            // country, currency, shops, refresh interval, theme
set_api_key(service: String, key: String)    -> ()                  // → OS keychain
import_steam_library(steam_id: String)       -> ImportReport
export_backup(path) / import_backup(path)
get_stats()                                  -> Stats
```

Events pushed Rust → Vue: `prices:updated`, `refresh:progress`, `deal:alert`.

---

## 7. UI

Dark-first, cover-art-led. Covers do the visual work; chrome stays out of the way.

- **Library (home).** Segmented control: `All · Wishlist · Backlog · Playing · Finished`.
  Board mode = 4 draggable columns; Grid mode = dense poster wall. Cards are 2:3 covers with
  a gradient scrim, title on hover, a status ring, and a **deal badge** (`−50%`) when a
  wishlist game is discounted. Sort by added / release / price / discount / name.
- **Detail drawer.** Slides in from the right over a blurred, color-extracted backdrop taken
  from the game's artwork. Top: hero art + cover + title + release + IGDB rating.
  Then genre/platform chips, summary. Then *Your copy* (status toggle group, owned-on platform,
  hours, rating, notes, target price). Then **Deals**, grouped by platform family (PC / PlayStation): one row per store with logo, price,
  strikethrough regular price, cut %, sale countdown, "all-time low €2.99 · 12 Apr 2026",
  and a 1-year sparkline. Footer: "Prices by IsThereAnyDeal · CheapShark".
- **Deals view.** Everything on the wishlist currently discounted, ranked by
  (% off) × (how close to all-time low) × your priority. Big "buy" affordance that also
  offers "mark as bought" afterwards.
- **Stats view.** Backlog size & value, hours played, genre donut, money spent,
  **money saved by waiting** (`price_at_add` − `purchase_price`), longest-waiting wishlist item.
- **Settings.** API keys (with a "test connection" button each), country/currency,
  enabled stores, refresh interval, notification toggles, backup import/export.
- **Cmd/Ctrl+K palette** — jump to a game, add a game, switch view.

Cover images are hotlinked from `images.igdb.com` on first paint and cached to disk so the
app works offline afterwards.

---

## 8. Build phases

**Phase 0 — scaffold** *(no APIs yet)* — ✅ **done**
- ✅ Tauri 2 + Vue 3 + TS + Vite 8, Tailwind v4, Pinia 4, Vue Router 5. Node pinned to 22 via `.nvmrc`.
- ✅ SQLite (`rusqlite`, bundled) with a `PRAGMA user_version` migration runner; migration 001 creates
  `setting`, `game`, `genre`, `platform`, the two join tables and `entry`. WAL enabled.
- ✅ `.env` credential loading with a generated template, `0600` permissions, and hot reload.
- ✅ App shell: sidebar, four views, dark tokens; Settings fully functional.
- ✅ Verified: window opens, `.env` created, migration 1 applied, settings round-trip through IPC.

Notes from the build:
- `vue-tsc` 3.3 still loads `typescript/lib/tsc`, which the TypeScript 7 native port removed —
  TypeScript is pinned to `~5.9` until vue-tsc catches up.
- Vite's dev server restarts on any `.env` change; `server.watch.ignored` now excludes it, so
  editing a key no longer reloads the app.
- `RouterLink`'s `active-class` matches by prefix, which kept the `/` link lit on every route.
  All nav links use `exact-active-class`.

**Phase 1 — the tracker itself** *(IGDB only)* — ✅ **done**
- ✅ Twitch client-credentials flow, in-memory token cache (renewed an hour early),
  401 → re-auth-and-retry, and a 4 req/s reservation limiter.
- ✅ `search_games` (with re-ranking), `get_game`, `list/add/update/reorder/delete_entries`.
- ✅ Board with four draggable columns, grid mode, filter, detail drawer, add-game search modal.
- ✅ Covers hotlinked from images.igdb.com (allowed by the CSP) with a per-title
  gradient placeholder when a game has no cover or the CDN fails.
- ✅ 11 unit tests + 3 live API tests (`cargo test --lib live -- --ignored`).

Notes from the build — IGDB's schema moved under us:
- **`category` is now `game_type`** and **`external_games.category` is now
  `external_game_source`**. IGDB *silently drops unknown field names* instead of erroring,
  so the old names read as permanently-missing data. The live tests exist to catch exactly
  this: they assert the Steam appid still resolves and the credits still arrive.
- Filtering to `game_type = (0,4,8,9,10)` is what keeps an unofficial Vita **mod** from
  outranking the real Hollow Knight.
- IGDB's relevance order alone is not good enough — it puts "Elden Ring Nightreign" above
  "Elden Ring". Results are re-ranked by `ln(rating_count)` plus an exact/prefix title bonus.

Bugs found after the first hands-on test (drag snapped back, drawer X dead):
- **Drag-and-drop snapped back.** Two causes. The board bound columns one-way with
  `:model-value`, so SortableJS moved the DOM and Vue's next render restored it from state
  that had not changed. And a cross-column drag emits **twice** — `removed` on the source,
  `added` on the destination — so two handlers raced, with the source one computing an order
  from stale state that dropped the moved entry entirely. Now each column owns a mutable
  array via `v-model`, and `resolveChange` ignores `removed` so only the destination acts.
- **The drawer's ✕ did nothing.** The title block is `absolute inset-x-0 bottom-0` and 112px
  tall inside a 128px hero, so it covered the button — and being later in the DOM, it won the
  hit test. It holds nothing clickable, so it (and the gradient) are now `pointer-events-none`,
  with the button at `z-20`.
- Drag felt clunky because it used the webview's native HTML5 drag image. SortableJS now runs
  in `forceFallback` mode with a styled clone (lift, tilt, shadow) and a dashed drop target,
  and Tauri's `dragDropEnabled` is off so its file-drop handler cannot interfere.

Bugs found and fixed during verification:
- **`Option<Option<T>>` collapsed `null` to "absent"**, so clearing a rating, note, target
  price or platform silently did nothing. Fixed with a `double_option` deserializer; two
  tests now pin absent / null / value as three distinct states.
- **Drag-and-drop emitted a duplicate id.** SortableJS only reports the destination column,
  so the source column's stale copy was still counted and the dragged game got two
  conflicting priorities. Drop resolution is now a pure `planDrop` function.
- Opening the detail drawer crushed the board columns to unreadable slivers; columns now
  hold a 236px minimum and the board scrolls sideways.
- Platform lists sorted alphabetically put **Linux** ahead of Windows, so a card's platform
  chip read "Linux" for a game everyone plays on PC. Each family now leads with its flagship.

**Phase 2 — prices & deals** *(CheapShark then ITAD)* — ✅ **done**, plus the shop whitelist
and storewide sale calendar added afterwards on request.
- ✅ Migration 003: `price_snapshot`, `price_low`, `price_history`, `alert_log`.
- ✅ CheapShark client (keyless, descriptive User-Agent) and ITAD client
  (lookup by appid → prices/v3 → historylow/v1 → history/v2).
- ✅ Deals panel in the drawer, downsampled sparkline, live sale countdown, Deals view
  ranked by closeness to the all-time low, and manual price entry.
- ✅ Background refresher (wishlist only, interval from Settings, 90s startup grace) with
  desktop notifications, deduplicated per (game, shop, price).
- ✅ Verified live: 20 shops in EUR for Disco Elysium, best €9.49 (−75%) at GOG against a
  €3.59 all-time low, "ends in 9d 19h" from a real ITAD expiry, cached re-read in 3–8 ms.

Notes from the build:
- **`since` is not optional on `history/v2`.** Without it ITAD returns roughly three months
  (12 entries for Hollow Knight); asking for five years returns 300–800. Everything derived
  from history is worthless on the default window.
- **CheapShark is USD-only** — no country or currency parameter exists. It is the
  no-key fallback, and anything it returns is labelled as USD with a note explaining how to
  get local prices. ITAD supersedes it as soon as a key is present.
- ITAD's history interleaves **every shop**, so the raw series is not a price timeline for
  the game. It is now collapsed to one point per day holding the cheapest offer that day.
- **Shops are filtered to the ones the user actually buys from** — Steam, GOG, Epic, Microsoft
  and (from Phase 2b) PlayStation. ITAD returns twenty-odd sellers per game and the grey-market
  keysellers bury the four that matter. An empty whitelist means *not chosen* and falls back to
  the defaults, which also upgrades installs written before the setting existed.
- The best offer must be chosen **after** filtering, not before: picking the cheapest across
  all shops first would drop a game from the Deals view whenever its lowest price happened to
  sit at an excluded seller.

**Phase 2b — PlayStation prices** *(PS Store GraphQL + manual fallback)* — ✅ **done**
- ✅ `clients/ps_store.rs` using `metGetPricingDataByConceptId`, the Apollo CSRF headers
  and `x-psn-store-locale-override` for region.
- ✅ Hashes live in `ps-store-queries.json` (bundled, overridable next to `.env`), never
  compiled in — a rotation is an edit, not a rebuild. A `not whitelisted` response is
  translated into exactly that instruction.
- ✅ Manual price entry for any platform, kept out of the automatic replace so a refresh
  never overwrites it, and exempt from the shop whitelist.
- ✅ Verified live: Cyberpunk 2077 €19.99 (−60%) and GTA V €9.99 (−50%) on PlayStation
  alongside PC rows, all in EUR.

Notes from the build:
- **Concept ids come free from IGDB.** `external_game_source = 36` is the PlayStation Store
  and its uid is the numeric concept id (Elden Ring = 10000333) — exactly what the pricing
  operation takes. No separate PS Store search was needed, and it is the same
  fetch-once-and-cache pattern as the Steam appid.
- Money arrives as **integer minor units** (`basePriceValue: 5999`) beside a localised
  string; the integers avoid parsing "€59,99" per region.
- PC and PlayStation are refreshed independently and merged. A PlayStation failure must not
  discard the PC prices, and the wholesale snapshot replacement is scoped to
  `source IN ('itad','cheapshark')` so PlayStation and manual rows survive it.
- **Some games have no current PC offers at all** — Skyrim and GTA V return nothing from
  ITAD, because the shops now sell Special/Enhanced editions as separate listings while the
  original entry lingers. A title-lookup fallback was tried and returns the same, so the
  panel says so plainly and points at manual entry instead of showing a bare empty list.
- Deals panel gains a per-platform grouping: `PC` rows and `PlayStation` rows side by side,
  each with its own best price, cut and store link.
- `own_platform` on an entry drives which group is highlighted.
- ✅ Done when: a game on both PC and PS5 shows both prices, and the month's request budget
  is nowhere near exhausted.

**Phase 3 — reach** — ✅ **done except Nintendo**
- ✅ **Steam library import.** Resolve a SteamID64 or vanity name (a pasted profile URL works),
  preview the account, choose played / never-launched, then import with playtime.
- ✅ **Stats view**: counts, hours, spend, wishlist value at today's prices, genre breakdown,
  longest wait, and "what waiting saved".
- ✅ **Cmd/Ctrl+K command palette** — jump to a game or run a command from anywhere.
- ✅ Sale calendar — delivered early, in Phase 2.
- ❌ **Nintendo eShop prices** — deferred, see below.

Notes from the build:
- **IGDB supplies the Steam appid mapping in bulk.** `external_games` accepts
  `uid = ("123","456",…)`, so a 1000-game account resolves in a handful of requests rather
  than one per title. 40/40 of a test account's most-played games matched.
- The import is **additive and idempotent by construction**: an existing entry is never
  re-categorised and hand-entered playtime is never overwritten, so re-running only fills
  gaps. Verified by importing twice — the second run added 0 and updated 0.
- Only `playing` is inferred, from `playtime_2weeks`. **`finished` is never guessed**, because
  Steam cannot know it.
- Importing everything is rarely what anyone wants — a 1088-game account would bury a curated
  backlog — so the preview reports played vs never-launched and the choice is explicit.
  Selecting neither imports nothing rather than defaulting to everything.
- `price_at_add` had never been populated, so "saved by waiting" could not work. It is now
  stamped the first time prices are seen for a wishlist game; nothing can reconstruct it later.
- Settings patches needed the same `double_option` treatment as entry patches so a stored
  Steam ID can actually be cleared.

**Nintendo eShop prices — deferred, and why**

IGDB has **no Nintendo external-game source** (its catalogue lists Steam, GOG, Epic, Microsoft,
PlayStation and others, but nothing for the eShop), so the `nsuid` that
`api.ec.nintendo.com/v1/price` needs cannot be derived the way the Steam appid and PlayStation
concept id both were. It would need a separate eShop title-search integration against an
undocumented Algolia index — noticeably more work and more fragile than either of the sources
already built, for the platform explicitly called out as least important.

Nintendo is not unserved in the meantime: manual price entry offers "Nintendo eShop" as a
platform, and those prices survive every automatic refresh.

**Phase 4 — ship** — ✅ **done except the updater**
- ✅ App icon: the same 2×2 mark as the sidebar, generated from a committed
  `icon-source.png` so the set can be rebuilt.
- ✅ `tauri build` verified end to end on macOS: a 19 MB `.app` and a 6.7 MB compressed
  `.dmg`, correct identifier and version, and the release binary launches and reads the
  existing database.
- ✅ JSON backup export/import, additive on the way in.
- ✅ GitHub Actions: typecheck, clippy (`-D warnings`), rustfmt and tests on macOS and
  Windows; installers bundled only on a tag or manual run.
- ❌ Auto-updater — deferred, see below.

Notes from the build:
- Builds are **unsigned**. Signing needs a paid Apple Developer certificate and a Windows
  code-signing certificate; macOS ad-hoc signs the bundle and Gatekeeper will still warn on
  first launch. Documented in the README rather than papered over.
- **An installed build does not read the project's `.env`** — the dev-tree lookup is behind
  `cfg!(debug_assertions)`, so a packaged app uses its own config directory and writes a
  blank template there on first run. Correct behaviour (a shipped app has no business reading
  a source tree) but a real first-install step, so it is called out in the README.
- Adding `-D warnings` to CI meant actually fixing every clippy lint first, including
  factoring the price-history tuple into a named `HistoryRow` type.

**Auto-updater — deferred, and why**

`tauri-plugin-updater` needs two things this project does not have: a signing keypair whose
private half must be kept secret, and somewhere to host a signed update manifest. Both are
reasonable for a distributed app and disproportionate for a personal one, where
`git pull && npm run app:build` is the update mechanism. Worth revisiting only if the app is
ever shared with other people.

---

## 9. Things to sort out before/while building

1. **Rust is too old.** Installed: `rustc 1.79.0` (June 2024). Current `tauri-cli` pulls deps
   with MSRV up to 1.88. → `rustup update stable` first. Node 21.1.0 is fine (Vite 7 prefers
   ≥20.19, so a bump to Node 22 LTS is worth doing too).
2. **Three accounts to register** (all free, all fine for non-commercial):
   Twitch dev app (IGDB Client ID + Secret) · IsThereAnyDeal API key · optional Steam Web API key.
   The app must degrade gracefully when they're missing — CheapShark carries Phase 2 alone.
3. **`.env` is plaintext, by choice.** Keys sit in a `0600` file rather than the OS keychain —
   readable by any process running as you, unlike Keychain/Credential Manager which gates
   per-application access. For free, non-commercial, rate-limited hobby keys that is a fair
   trade for being able to edit one file. Worth revisiting if a key ever gains billing rights.
4. **Attribution is a licence condition**, not a nicety: ITAD requires a link/credit and
   forbids stripping affiliate tags from the URLs it returns; PlatPrices requires
   "Powered by PlatPrices". Build these into the deals panel from the start.
5. **Steam appid is the universal join key.** Get it from IGDB `external_games`, and prefer
   it over title matching everywhere — title lookup gets remasters and editions wrong.
6. **Undocumented endpoints** (Nintendo `api.ec.nintendo.com`, Steam `appdetails`) can change
   without warning. Isolate each behind its own client module with a feature flag so a
   breakage degrades one row of the deals panel instead of the app.
7. **Region matters.** Country/currency is a first-class setting (defaulting to DE/EUR);
   prices differ per region and ITAD/PlatPrices both take a country parameter.

## 10. Decisions taken

- **Platforms: PC and PlayStation are first-class**, Nintendo Switch is a feature-flagged extra,
  **Xbox is out of scope**. This is why PlatPrices moved from "optional Phase 3" into Phase 2b
  and why the deals panel groups prices by platform family from the start.
- **Steam library import is in** (Phase 3), additive and re-runnable, needs a free Steam Web API
  key and a public profile. Key is in place.
- **Credentials are in `.env`**: IGDB, IsThereAnyDeal and Steam keys are all present and verified.
  PlatPrices was never issued — see the PlayStation row in §3.
- **Region defaults to Germany / EUR**, changeable in Settings; every price call is
  country-parameterised.
