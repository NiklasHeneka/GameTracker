<script setup lang="ts">
import { ref } from "vue";
import { storeToRefs } from "pinia";

import PageHeader from "@/components/PageHeader.vue";
import AppIcon from "@/components/AppIcon.vue";
import SteamImport from "@/components/SteamImport.vue";
import { useSettingsStore } from "@/stores/settings";

const store = useSettingsStore();
const { settings, config, loading, error } = storeToRefs(store);

const COUNTRIES = [
  { code: "DE", label: "Germany" },
  { code: "AT", label: "Austria" },
  { code: "CH", label: "Switzerland" },
  { code: "GB", label: "United Kingdom" },
  { code: "US", label: "United States" },
  { code: "FR", label: "France" },
  { code: "NL", label: "Netherlands" },
  { code: "PL", label: "Poland" },
];
const CURRENCIES = ["EUR", "USD", "GBP", "CHF", "PLN"];

// Kept in step with DEFAULT_SHOPS in the Rust settings module.
const SHOPS = ["Steam", "GOG", "Epic Game Store", "Microsoft Store", "PlayStation Store"];

function toggleShop(shop: string, on: boolean) {
  const current = settings.value?.enabledShops ?? [];
  const next = on ? [...current, shop] : current.filter((s) => s !== shop);
  // An empty list is read as "not chosen" and falls back to every default, so
  // keep at least one shop selected rather than silently showing them all.
  store.patch({ enabledShops: next.length ? next : current });
}

const reloading = ref(false);
const notice = ref<string | null>(null);

async function reloadKeys() {
  reloading.value = true;
  notice.value = null;
  try {
    await store.reload();
    notice.value = config.value?.ready
      ? "Credentials loaded."
      : "Reloaded — still missing a required key.";
  } catch (e) {
    notice.value = (e as { message?: string }).message ?? String(e);
  } finally {
    reloading.value = false;
  }
}

async function reveal() {
  try {
    await store.reveal();
  } catch (e) {
    notice.value = (e as { message?: string }).message ?? String(e);
  }
}
</script>

<template>
  <PageHeader title="Settings" subtitle="Credentials, region, and refresh behaviour" />

  <div class="pane-scroll flex-1">
    <div class="mx-auto max-w-2xl px-8 py-7">
      <p v-if="loading" class="text-[13px] text-ink-faint">Loading…</p>

      <!-- A failed load must not be a dead end: settings are how the user
           fixes a broken configuration in the first place. -->
      <div
        v-else-if="error"
        class="flex items-start gap-3 rounded-xl border border-line bg-surface px-4 py-3.5"
      >
        <span class="mt-0.5 grid h-6 w-6 shrink-0 place-items-center rounded-md bg-sale/15 text-sale">
          <AppIcon name="alert" :size="13" />
        </span>
        <div class="min-w-0 flex-1">
          <p class="text-[13px]">Could not load settings</p>
          <p class="mt-0.5 break-words text-[12px] text-ink-faint">{{ error }}</p>
        </div>
        <button
          type="button"
          class="shrink-0 rounded-lg border border-line bg-elevated px-2.5 py-1.5 text-[12.5px] text-ink-dim transition-colors hover:text-ink"
          @click="store.load()"
        >
          Try again
        </button>
      </div>

      <template v-else-if="settings && config">
        <!-- ── Credentials ─────────────────────────────────────────────── -->
        <section>
          <div class="flex items-baseline justify-between gap-4">
            <h2 class="text-[14px] font-semibold">API credentials</h2>
            <span
              class="rounded-full px-2 py-0.5 text-[11px] font-medium"
              :class="
                config.ready ? 'bg-deal-soft text-deal' : 'bg-sale/15 text-sale'
              "
            >
              {{ config.ready ? "Ready" : "Setup needed" }}
            </span>
          </div>
          <p class="mt-1.5 text-[12.5px] leading-relaxed text-ink-faint">
            Keys are read from a <code class="text-ink-dim">.env</code> file by the Rust
            backend. They are never sent to the interface, so nothing here can leak them
            into the page.
          </p>

          <div class="mt-4 overflow-hidden rounded-xl border border-line bg-surface">
            <div
              v-for="(k, i) in config.keys"
              :key="k.name"
              class="flex items-center gap-3 px-4 py-3"
              :class="i > 0 && 'border-t border-line'"
            >
              <span
                class="grid h-6 w-6 shrink-0 place-items-center rounded-md"
                :class="
                  k.present
                    ? 'bg-deal-soft text-deal'
                    : k.required
                      ? 'bg-sale/15 text-sale'
                      : 'bg-elevated text-ink-faint'
                "
              >
                <AppIcon :name="k.present ? 'check' : k.required ? 'alert' : 'key'" :size="13" />
              </span>

              <div class="min-w-0 flex-1">
                <p class="truncate text-[13px]">{{ k.label }}</p>
                <p class="mt-0.5 font-mono text-[11px] text-ink-faint">{{ k.name }}</p>
              </div>

              <span v-if="k.present" class="font-mono text-[11.5px] text-ink-dim">{{ k.hint }}</span>
              <span v-else class="text-[11.5px]" :class="k.required ? 'text-sale' : 'text-ink-faint'">
                {{ k.required ? "required" : "optional" }}
              </span>
            </div>
          </div>

          <div class="mt-3 flex flex-wrap items-center gap-2">
            <button
              type="button"
              class="flex items-center gap-1.5 rounded-lg border border-line bg-elevated px-2.5 py-1.5 text-[12.5px] text-ink-dim transition-colors hover:text-ink"
              @click="reveal"
            >
              <AppIcon name="folder" :size="14" />
              Show .env
            </button>
            <button
              type="button"
              :disabled="reloading"
              class="flex items-center gap-1.5 rounded-lg border border-line bg-elevated px-2.5 py-1.5 text-[12.5px] text-ink-dim transition-colors hover:text-ink disabled:opacity-50"
              @click="reloadKeys"
            >
              <AppIcon name="refresh" :size="14" />
              {{ reloading ? "Reloading…" : "Reload keys" }}
            </button>
            <span v-if="notice" class="text-[12px] text-ink-faint">{{ notice }}</span>
          </div>

          <p class="mt-2.5 truncate font-mono text-[11px] text-ink-faint" :title="config.envPath">
            {{ config.envPath }}
          </p>
        </section>

        <!-- ── Region ──────────────────────────────────────────────────── -->
        <section class="mt-9">
          <h2 class="text-[14px] font-semibold">Region</h2>
          <p class="mt-1.5 text-[12.5px] leading-relaxed text-ink-faint">
            Every price source is region-specific — this decides which store prices you see.
          </p>

          <div class="mt-4 grid grid-cols-2 gap-3">
            <label class="block">
              <span class="mb-1.5 block text-[12px] text-ink-dim">Country</span>
              <select
                class="w-full rounded-lg border border-line bg-surface px-2.5 py-2 text-[13px]"
                :value="settings.country"
                @change="store.patch({ country: ($event.target as HTMLSelectElement).value })"
              >
                <option v-for="c in COUNTRIES" :key="c.code" :value="c.code">
                  {{ c.label }} ({{ c.code }})
                </option>
              </select>
            </label>

            <label class="block">
              <span class="mb-1.5 block text-[12px] text-ink-dim">Currency</span>
              <select
                class="w-full rounded-lg border border-line bg-surface px-2.5 py-2 text-[13px]"
                :value="settings.currency"
                @change="store.patch({ currency: ($event.target as HTMLSelectElement).value })"
              >
                <option v-for="c in CURRENCIES" :key="c" :value="c">{{ c }}</option>
              </select>
            </label>
          </div>
        </section>

        <!-- ── Stores ──────────────────────────────────────────────────── -->
        <section class="mt-9">
          <h2 class="text-[14px] font-semibold">Stores</h2>
          <p class="mt-1.5 text-[12.5px] leading-relaxed text-ink-faint">
            Which shops to show prices from. Price sources list dozens of resellers;
            only these appear in the deals panel.
          </p>

          <div class="mt-4 flex flex-wrap gap-1.5">
            <button
              v-for="shop in SHOPS"
              :key="shop"
              type="button"
              class="rounded-lg border px-2.5 py-1.5 text-[12.5px] transition-colors"
              :class="
                settings.enabledShops.includes(shop)
                  ? 'border-accent/40 bg-accent-soft text-accent-ink'
                  : 'border-line bg-surface text-ink-faint hover:text-ink'
              "
              @click="toggleShop(shop, !settings.enabledShops.includes(shop))"
            >
              {{ shop }}
            </button>
          </div>
          <p class="mt-2 text-[11px] text-ink-faint">
            PlayStation prices arrive with the PlayStation Store integration; the shop is
            listed here so it is ready when they do.
          </p>
        </section>

        <!-- ── Price tracking ──────────────────────────────────────────── -->
        <section class="mt-9">
          <h2 class="text-[14px] font-semibold">Price tracking</h2>

          <div class="mt-4 divide-y divide-line overflow-hidden rounded-xl border border-line bg-surface">
            <label class="flex items-center gap-4 px-4 py-3">
              <span class="min-w-0 flex-1">
                <span class="block text-[13px]">Check prices every</span>
                <span class="mt-0.5 block text-[11.5px] text-ink-faint">
                  Wishlist entries only, in the background.
                </span>
              </span>
              <select
                class="rounded-lg border border-line bg-elevated px-2.5 py-1.5 text-[12.5px]"
                :value="settings.refreshIntervalHours"
                @change="
                  store.patch({
                    refreshIntervalHours: Number(($event.target as HTMLSelectElement).value),
                  })
                "
              >
                <option v-for="h in [1, 3, 6, 12, 24]" :key="h" :value="h">{{ h }} h</option>
              </select>
            </label>

            <label class="flex items-center gap-4 px-4 py-3">
              <span class="min-w-0 flex-1">
                <span class="block text-[13px]">Notify me on price drops</span>
                <span class="mt-0.5 block text-[11.5px] text-ink-faint">
                  A desktop notification when a wishlist game falls below your target price.
                </span>
              </span>
              <input
                type="checkbox"
                class="h-4 w-4 accent-accent"
                :checked="settings.notificationsEnabled"
                @change="
                  store.patch({
                    notificationsEnabled: ($event.target as HTMLInputElement).checked,
                  })
                "
              />
            </label>

            <label class="flex items-center gap-4 px-4 py-3">
              <span class="min-w-0 flex-1">
                <span class="block text-[13px]">PlayStation prices</span>
                <span class="mt-0.5 block text-[11.5px] text-ink-faint">
                  Via PlatPrices. Free tier is 1000 requests per month, so only wishlist
                  entries are refreshed.
                </span>
              </span>
              <input
                type="checkbox"
                class="h-4 w-4 accent-accent"
                :checked="settings.trackPlaystation"
                @change="
                  store.patch({ trackPlaystation: ($event.target as HTMLInputElement).checked })
                "
              />
            </label>

            <label class="flex items-center gap-4 px-4 py-3">
              <span class="min-w-0 flex-1">
                <span class="block text-[13px]">Nintendo eShop prices</span>
                <span class="mt-0.5 block text-[11.5px] text-ink-faint">
                  Uses an undocumented Nintendo endpoint. Off by default — if it breaks it
                  only affects this one row of the deals panel.
                </span>
              </span>
              <input
                type="checkbox"
                class="h-4 w-4 accent-accent"
                :checked="settings.trackNintendo"
                @change="
                  store.patch({ trackNintendo: ($event.target as HTMLInputElement).checked })
                "
              />
            </label>
          </div>
        </section>

        <SteamImport />

        <p class="mt-8 text-[11.5px] leading-relaxed text-ink-faint">
          Game metadata by IGDB. Prices by IsThereAnyDeal and CheapShark.
        </p>
      </template>
    </div>
  </div>
</template>
