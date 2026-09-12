import { createApp } from "vue";
import { createPinia } from "pinia";

import App from "./App.vue";
import { router } from "./router";
import "./styles/main.css";

// `titleBarStyle: "Overlay"` is macOS-only: there the traffic lights float over
// the app and the top of the window has to be kept clear, while Windows draws a
// real title bar and the same inset would just be dead space. One attribute
// lets the stylesheet handle both.
if (navigator.userAgent.includes("Macintosh")) {
  document.documentElement.dataset.platform = "macos";
}

createApp(App).use(createPinia()).use(router).mount("#app");
