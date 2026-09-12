import type { EntryPatch, LibraryEntry } from "@/types/models";

/**
 * The board's four columns. Ownership and progress are separate columns in the
 * database, so each bucket both *matches* entries and knows the patch that
 * *moves* an entry into it when dropped.
 */
export interface Bucket {
  key: string;
  label: string;
  hint: string;
  matches: (e: LibraryEntry) => boolean;
  patch: EntryPatch;
}

export const BUCKETS: Bucket[] = [
  {
    key: "wishlist",
    label: "Wishlist",
    hint: "Want it, don't own it",
    matches: (e) => !e.owned && e.status === "want",
    patch: { owned: false, status: "want" },
  },
  {
    key: "backlog",
    label: "Backlog",
    hint: "Own it, haven't played it",
    matches: (e) => e.owned && e.status === "want",
    patch: { owned: true, status: "want" },
  },
  {
    key: "playing",
    label: "Playing",
    hint: "In progress",
    matches: (e) => e.status === "playing",
    // You cannot be playing a game you do not own.
    patch: { owned: true, status: "playing" },
  },
  {
    key: "finished",
    label: "Finished",
    hint: "Done with it",
    matches: (e) => e.status === "finished" || e.status === "dropped",
    patch: { owned: true, status: "finished" },
  },
];

export function bucketOf(entry: LibraryEntry): Bucket {
  return BUCKETS.find((b) => b.matches(entry)) ?? BUCKETS[0]!;
}

/** vuedraggable's `change` payload. Exactly one key is present per event. */
export interface DraggableChange {
  added?: { element: LibraryEntry; newIndex: number };
  removed?: { element: LibraryEntry; oldIndex: number };
  moved?: { element: LibraryEntry; oldIndex: number; newIndex: number };
}

export interface BoardColumnState {
  bucket: Bucket;
  entries: LibraryEntry[];
}

export interface ChangePlan {
  /** The entry that landed in a new column, and the patch that moves it. */
  patch: { id: number; patch: EntryPatch } | null;
  /** Every entry id in board order, for persisting `priority`. */
  order: number[];
}

/**
 * Decide what a vuedraggable `change` means.
 *
 * A cross-column drag emits twice — `removed` on the source and `added` on the
 * destination. Only the destination event does anything; handling both would
 * patch the same entry twice and race over the resulting order.
 *
 * `null` means the event needs no work at all.
 */
export function resolveChange(
  board: BoardColumnState[],
  target: Bucket,
  evt: DraggableChange,
): ChangePlan | null {
  if (evt.removed) return null;

  return {
    patch: evt.added ? { id: evt.added.element.id, patch: target.patch } : null,
    // vuedraggable has already rearranged the arrays, so the board is the
    // authority on the new order.
    order: board.flatMap((c) => c.entries.map((e) => e.id)),
  };
}
