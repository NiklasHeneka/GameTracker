import { createRouter, createWebHashHistory } from "vue-router";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "library", component: () => import("@/views/LibraryView.vue") },
    { path: "/play-next", name: "play-next", component: () => import("@/views/PlayNextView.vue") },
    { path: "/deals", name: "deals", component: () => import("@/views/DealsView.vue") },
    { path: "/stats", name: "stats", component: () => import("@/views/StatsView.vue") },
    { path: "/settings", name: "settings", component: () => import("@/views/SettingsView.vue") },
    { path: "/:pathMatch(.*)*", redirect: "/" },
  ],
});
