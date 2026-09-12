<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { useRouter } from "vue-router";

import AppIcon from "./AppIcon.vue";
import GameCover from "./GameCover.vue";
import { useLibraryStore } from "@/stores/library";
import type { LibraryEntry } from "@/types/models";

const emit = defineEmits<{ addGame: [] }>();

const router = useRouter();
const library = useLibraryStore();

const open = ref(false);
const term = ref("");
const active = ref(0);
const input = ref<HTMLInputElement | null>(null);

interface Command {
  id: string;
  label: string;
  hint?: string;
  icon: string;
  run: () => void;
  entry?: LibraryEntry;
}

const commands = computed<Command[]>(() => [
  { id: "nav-library", label: "Library", icon: "library", run: () => router.push("/") },
  { id: "nav-deals", label: "Deals", icon: "tag", run: () => router.push("/deals") },
  { id: "nav-stats", label: "Stats", icon: "chart", run: () => router.push("/stats") },
  { id: "nav-settings", label: "Settings", icon: "gear", run: () => router.push("/settings") },
  {
    id: "add",
    label: "Add a game",
    hint: "search IGDB",
    icon: "plus",
    run: () => emit("addGame"),
  },
]);

const results = computed(() => {
  const q = term.value.trim().toLowerCase();

  // Games first when searching — jumping to a title is the common case.
  const games: Command[] = q
    ? library.entries
        .filter((e) => e.game.name.toLowerCase().includes(q))
        .slice(0, 6)
        .map((e) => ({
          id: `game-${e.id}`,
          label: e.game.name,
          hint: e.owned ? "owned" : "wishlist",
          icon: "library",
          entry: e,
          run: () => {
            router.push("/");
            library.select(e.id);
          },
        }))
    : [];

  const actions = commands.value.filter((c) => !q || c.label.toLowerCase().includes(q));
  return [...games, ...actions];
});

watch(results, () => (active.value = 0));

function show() {
  open.value = true;
  term.value = "";
  active.value = 0;
  nextTick(() => input.value?.focus());
}

function run(command: Command) {
  open.value = false;
  command.run();
}

function onKeydown(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
    e.preventDefault();
    open.value ? (open.value = false) : show();
    return;
  }
  if (!open.value) return;

  if (e.key === "Escape") {
    open.value = false;
  } else if (e.key === "ArrowDown") {
    e.preventDefault();
    active.value = (active.value + 1) % Math.max(1, results.value.length);
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    active.value = (active.value - 1 + results.value.length) % Math.max(1, results.value.length);
  } else if (e.key === "Enter") {
    const chosen = results.value[active.value];
    if (chosen) run(chosen);
  }
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-[60] flex items-start justify-center bg-black/55 px-6 pt-[16vh] backdrop-blur-sm"
    @click.self="open = false"
  >
    <div
      class="w-full max-w-lg overflow-hidden rounded-2xl border border-line-solid bg-surface shadow-2xl shadow-black/60"
      role="dialog"
      aria-label="Command palette"
    >
      <div class="flex items-center gap-2.5 border-b border-line px-4">
        <AppIcon name="search" :size="16" class="shrink-0 text-ink-faint" />
        <input
          ref="input"
          v-model="term"
          type="text"
          placeholder="Jump to a game, or run a command…"
          class="w-full bg-transparent py-3 text-[13.5px] outline-none placeholder:text-ink-faint"
        />
      </div>

      <ul v-if="results.length" class="max-h-80 overflow-y-auto py-1">
        <li v-for="(item, i) in results" :key="item.id">
          <button
            type="button"
            class="flex w-full items-center gap-2.5 px-4 py-2 text-left transition-colors"
            :class="i === active ? 'bg-elevated' : 'hover:bg-elevated/60'"
            @click="run(item)"
            @mousemove="active = i"
          >
            <span
              v-if="item.entry"
              class="h-8 w-6 shrink-0 overflow-hidden rounded bg-elevated"
            >
              <GameCover
                :image-id="item.entry.game.coverImageId"
                :name="item.entry.game.name"
                size="cover_small"
              />
            </span>
            <AppIcon v-else :name="item.icon" :size="16" class="shrink-0 text-ink-faint" />

            <span class="min-w-0 flex-1 truncate text-[13px]">{{ item.label }}</span>
            <span v-if="item.hint" class="shrink-0 text-[11px] text-ink-faint">
              {{ item.hint }}
            </span>
          </button>
        </li>
      </ul>

      <p v-else class="px-4 py-6 text-center text-[12.5px] text-ink-faint">
        Nothing matches “{{ term }}”.
      </p>

      <p class="border-t border-line px-4 py-2 text-[11px] text-ink-faint">
        ↑↓ to move · ↵ to open · esc to close
      </p>
    </div>
  </div>
</template>
