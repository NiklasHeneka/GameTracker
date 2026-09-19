/** Mirrors `settings::Settings` in the Rust backend. */
export interface Settings {
  country: string;
  currency: string;
  theme: string;
  refreshIntervalHours: number;
  notificationsEnabled: boolean;
  enabledShops: string[];
  trackPlaystation: boolean;
  trackNintendo: boolean;
  /** Resolved SteamID64, remembered after the first import. */
  steamId: string | null;
}

/**
 * `undefined` (key absent) leaves a field alone; an explicit `null` clears a
 * nullable one. The backend distinguishes the two.
 */
export type SettingsPatch = Partial<Settings>;

/** Mirrors `config::KeyStatus`. Values never cross the IPC boundary. */
export interface KeyStatus {
  name: string;
  label: string;
  required: boolean;
  present: boolean;
  /** e.g. `abcd…wxyz` — enough to identify the key, not enough to use it. */
  hint: string | null;
}

/** Mirrors `config::ConfigStatus`. */
export interface ConfigStatus {
  envPath: string;
  envExists: boolean;
  keys: KeyStatus[];
  ready: boolean;
}

/** Shape of every rejected `invoke`, from `error::AppError`. */
export interface IpcError {
  kind: "db" | "io" | "tauri" | "serde" | "config" | "not_found" | "unknown";
  message: string;
}

export interface PlatformRef {
  id: number;
  name: string;
  abbreviation: string | null;
  /** pc | playstation | nintendo | xbox | other */
  family: string;
}

/** Enough to draw a card. Mirrors `db::models::GameSummary`. */
export interface GameSummary {
  igdbId: number;
  name: string;
  coverImageId: string | null;
  firstRelease: number | null;
  steamAppid: number | null;
  genres: string[];
  platforms: PlatformRef[];
}

/** `GameSummary` fields are flattened into this by serde. */
export interface GameDetail extends GameSummary {
  summaryText: string | null;
  artworkImageId: string | null;
  igdbRating: number | null;
  developer: string | null;
  publisher: string | null;
}

export interface SearchResult {
  igdbId: number;
  name: string;
  coverImageId: string | null;
  releaseYear: number | null;
  platforms: string[];
  ratingCount: number;
  inLibrary: boolean;
}

export type EntryStatus = "want" | "playing" | "finished" | "dropped";

export interface LibraryEntry {
  id: number;
  game: GameSummary;
  owned: boolean;
  status: EntryStatus;
  priority: number;
  ownPlatform: string | null;
  targetPrice: number | null;
  priceAtAdd: number | null;
  purchasePrice: number | null;
  purchaseDate: number | null;
  purchaseStore: string | null;
  hoursPlayed: number | null;
  myRating: number | null;
  notes: string | null;
  addedAt: number;
  updatedAt: number;
  startedAt: number | null;
  finishedAt: number | null;
}

/**
 * Partial update. A key set to `null` clears the column; a key left out is
 * untouched — which is why the backend wraps these in a double Option.
 */
export interface EntryPatch {
  owned?: boolean;
  status?: EntryStatus;
  ownPlatform?: string | null;
  targetPrice?: number | null;
  purchasePrice?: number | null;
  purchaseDate?: number | null;
  purchaseStore?: string | null;
  hoursPlayed?: number | null;
  myRating?: number | null;
  notes?: string | null;
}

export interface PriceRow {
  shop: string;
  platformFamily: string;
  currency: string;
  price: number;
  regular: number | null;
  cut: number;
  url: string | null;
  /** Unix seconds when the sale ends; only ~half of deals publish this. */
  saleExpiry: number | null;
  source: "itad" | "cheapshark" | "manual";
  isAllTimeLow: boolean;
}

export interface PriceGroup {
  family: string;
  rows: PriceRow[];
}

export interface PriceLow {
  scope: "all" | "y1" | "m3";
  shop: string | null;
  currency: string;
  price: number;
  cut: number;
  occurredAt: number | null;
}

export interface LastSale {
  price: number;
  currency: string;
  cut: number;
  occurredAt: number;
  shop: string | null;
}

export interface HistoryPoint {
  ts: number;
  price: number;
  cut: number;
}

/**
 * How much of the observed window had a discount. Not a prediction — with many
 * shops running staggered sales, a popular game is discounted somewhere on most
 * days, so "when is the next sale" has no honest answer.
 */
export interface DiscountPattern {
  daysObserved: number;
  daysDiscounted: number;
  /** Share of observed days with any discount, 0-100. */
  sharePercent: number;
  deepestCut: number;
}

/** What a game cost during a previous run of a storewide sale. */
export interface PastEventPrice {
  event: string;
  start: number;
  end: number;
  bestPrice: number | null;
  bestCut: number | null;
  currency: string;
  /** False when no price was recorded then — different from "not discounted". */
  hadData: boolean;
}

/** The next known storewide sale at one shop. */
export interface StoreSaleOutlook {
  shop: string;
  event: string;
  nextStart: number;
  nextEnd: number;
  /** True when the store published the dates; otherwise estimated. */
  confirmed: boolean;
  liveNow: boolean;
  daysAway: number;
  lastEvent: PastEventPrice | null;
}

export interface PriceOverview {
  gameId: number;
  country: string;
  groups: PriceGroup[];
  lows: PriceLow[];
  lastSale: LastSale | null;
  history: HistoryPoint[];
  pattern: DiscountPattern | null;
  saleOutlook: StoreSaleOutlook[];
  fetchedAt: number | null;
  stale: boolean;
  note: string | null;
}

/**
 * One store's answer for one game: what it costs there now, and when that
 * store's next storewide sale is. Mirrors `commands::queue::QueueStore`.
 */
export interface QueueStore {
  shop: string;
  /** `null` when the store carries no listing for this game. */
  offer: PriceRow | null;
  outlook: StoreSaleOutlook | null;
}

/**
 * One row of the Play Next list. Mirrors `commands::queue::QueueRow`.
 *
 * Ordering lives in the `queue` table, not in `entry.priority`, so this list
 * and the board's columns can be arranged independently.
 */
export interface QueueRow {
  entry: LibraryEntry;
  /** `null` means "whichever is cheapest" — `stores` is already in that order. */
  preferredShop: string | null;
  /** Cheapest offer first. Always empty once the game is owned. */
  stores: QueueStore[];
  addedAt: number;
  fetchedAt: number | null;
  stale: boolean;
}

export interface DealRow {
  entryId: number;
  gameId: number;
  name: string;
  coverImageId: string | null;
  shop: string;
  currency: string;
  price: number;
  regular: number | null;
  cut: number;
  url: string | null;
  saleExpiry: number | null;
  allTimeLow: number | null;
  /** price ÷ all-time low; 1.0 means it is at its best price ever. */
  vsAllTimeLow: number | null;
  targetPrice: number | null;
  meetsTarget: boolean;
}

export interface RefreshReport {
  checked: number;
  updated: number;
  failed: number;
  alerts: number;
  message: string | null;
}

export interface SteamPreview {
  steamId: string;
  total: number;
  played: number;
  unplayed: number;
  /** Already tracked, so an import would only fill in playtime. */
  alreadyTracked: number;
}

export interface ImportOptions {
  includePlayed: boolean;
  includeUnplayed: boolean;
  /** Ignore anything under this many minutes (0 = no floor). */
  minMinutes: number;
}

export interface ImportReport {
  considered: number;
  matched: number;
  added: number;
  updated: number;
  unmatched: number;
  message: string | null;
}

export interface GenreCount {
  name: string;
  count: number;
}

export interface Stats {
  total: number;
  owned: number;
  wishlist: number;
  backlog: number;
  playing: number;
  finished: number;
  hoursPlayed: number;
  averageRating: number | null;
  currency: string;
  moneySpent: number;
  wishlistValue: number;
  savedByWaiting: number;
  /** How many purchases the saving is based on. */
  savedFrom: number;
  topGenres: GenreCount[];
  longestWait: { name: string; days: number } | null;
}

export interface ImportSummary {
  entriesInFile: number;
  added: number;
  /** Already tracked; left exactly as they were. */
  skipped: number;
  /** Play Next rows restored. */
  queued: number;
  exportedAt: number;
  settingsApplied: boolean;
}
