import { call } from "./ipc";
import type { EntryPatch, GameDetail, LibraryEntry, SearchResult } from "@/types/models";

export const searchGames = (query: string) => call<SearchResult[]>("search_games", { query });

export const getGame = (igdbId: number, refresh = false) =>
  call<GameDetail>("get_game", { igdbId, refresh });

export const listEntries = () => call<LibraryEntry[]>("list_entries");

export const addEntry = (igdbId: number, owned: boolean, ownPlatform: string | null = null) =>
  call<LibraryEntry>("add_entry", { igdbId, owned, ownPlatform });

export const updateEntry = (id: number, patch: EntryPatch) =>
  call<LibraryEntry>("update_entry", { id, patch });

export const reorderEntries = (ids: number[]) => call<void>("reorder_entries", { ids });

export const deleteEntry = (id: number) => call<void>("delete_entry", { id });
