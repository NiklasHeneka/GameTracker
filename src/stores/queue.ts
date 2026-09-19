import { defineStore } from "pinia";
import { computed, ref } from "vue";

import { dequeue, enqueue, listQueue, reorderQueue, setQueueShop } from "@/api/queue";
import type { LibraryEntry, QueueRow } from "@/types/models";

/**
 * The Play Next list. Deliberately a separate store from the library: the two
 * hold the same entries in different orders, and merging them would mean one
 * list's sort silently rewriting the other's.
 */
export const useQueueStore = defineStore("queue", () => {
  const rows = ref<QueueRow[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  const queuedIds = computed(() => new Set(rows.value.map((r) => r.entry.id)));

  function message(e: unknown) {
    return (e as { message?: string }).message ?? String(e);
  }

  async function load() {
    loading.value = true;
    error.value = null;
    try {
      rows.value = await listQueue();
    } catch (e) {
      error.value = message(e);
    } finally {
      loading.value = false;
    }
  }

  async function add(entryId: number) {
    const row = await enqueue(entryId);
    // The backend appends, and refuses to move something already queued, so
    // the local list only ever grows here. Assigned rather than pushed: the
    // page watches this array by reference to rebuild its draggable copy.
    if (!queuedIds.value.has(entryId)) rows.value = [...rows.value, row];
    return row;
  }

  async function remove(entryId: number) {
    const before = rows.value;
    rows.value = rows.value.filter((r) => r.entry.id !== entryId);
    try {
      await dequeue(entryId);
    } catch (e) {
      rows.value = before;
      error.value = message(e);
      throw e;
    }
  }

  /**
   * Persist a drag; `entryIds` is the whole list in its new order.
   *
   * Applied locally first so the row stays where it was dropped, then rolled
   * back wholesale if the write fails — a half-applied order would be worse
   * than either end state.
   */
  async function reorder(entryIds: number[]) {
    const byId = new Map(rows.value.map((r) => [r.entry.id, r]));
    const before = rows.value;
    rows.value = entryIds.flatMap((id) => {
      const row = byId.get(id);
      return row ? [row] : [];
    });
    try {
      await reorderQueue(entryIds);
    } catch (e) {
      rows.value = before;
      error.value = message(e);
      throw e;
    }
  }

  /** Remember which store's price a row shows. */
  async function setShop(entryId: number, shop: string | null) {
    const fresh = await setQueueShop(entryId, shop);
    rows.value = rows.value.map((r) => (r.entry.id === entryId ? fresh : r));
  }

  /**
   * Keep the embedded entries in step with the library store.
   *
   * A queue row carries its own copy of the entry, so marking a game as owned
   * from the drawer would otherwise leave this page showing the old state
   * until a reload.
   */
  function sync(entries: LibraryEntry[]) {
    const byId = new Map(entries.map((e) => [e.id, e]));
    rows.value = rows.value.flatMap((row) => {
      const fresh = byId.get(row.entry.id);
      // Gone from the library means gone from here too — the database drops
      // the row by cascade, and this mirrors that without a round trip.
      if (!fresh) return [];
      return [{ ...row, entry: fresh }];
    });
  }

  return { rows, loading, error, queuedIds, load, add, remove, reorder, setShop, sync };
});
