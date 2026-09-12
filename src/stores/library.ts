import { defineStore } from "pinia";
import { computed, ref } from "vue";

import {
  addEntry,
  deleteEntry,
  listEntries,
  reorderEntries,
  updateEntry,
} from "@/api/library";
import type { EntryPatch, LibraryEntry } from "@/types/models";

export const useLibraryStore = defineStore("library", () => {
  const entries = ref<LibraryEntry[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const selectedId = ref<number | null>(null);

  const selected = computed(() => entries.value.find((e) => e.id === selectedId.value) ?? null);
  const count = computed(() => entries.value.length);

  function message(e: unknown) {
    return (e as { message?: string }).message ?? String(e);
  }

  function replace(entry: LibraryEntry) {
    const i = entries.value.findIndex((e) => e.id === entry.id);
    if (i === -1) entries.value.push(entry);
    else entries.value[i] = entry;
  }

  async function load() {
    loading.value = true;
    error.value = null;
    try {
      entries.value = await listEntries();
    } catch (e) {
      error.value = message(e);
    } finally {
      loading.value = false;
    }
  }

  async function add(igdbId: number, owned: boolean) {
    const entry = await addEntry(igdbId, owned);
    replace(entry);
    return entry;
  }

  async function patch(id: number, p: EntryPatch) {
    const before = entries.value.find((e) => e.id === id);
    const snapshot = before ? { ...before } : null;

    // Apply locally first so dragging a card feels instant, then reconcile
    // with whatever the backend actually stored (it stamps timestamps).
    if (before) Object.assign(before, p);
    try {
      replace(await updateEntry(id, p));
    } catch (e) {
      if (snapshot) replace(snapshot);
      error.value = message(e);
      throw e;
    }
  }

  async function remove(id: number) {
    await deleteEntry(id);
    entries.value = entries.value.filter((e) => e.id !== id);
    if (selectedId.value === id) selectedId.value = null;
  }

  /** Persist a drag-reordering; `ids` is the new global order. */
  async function reorder(ids: number[]) {
    ids.forEach((id, position) => {
      const entry = entries.value.find((e) => e.id === id);
      if (entry) entry.priority = position;
    });
    await reorderEntries(ids);
  }

  function select(id: number | null) {
    selectedId.value = id;
  }

  return {
    entries, loading, error, selectedId, selected, count,
    load, add, patch, remove, reorder, select,
  };
});
