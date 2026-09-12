import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// Tauri expects a fixed port and must fail rather than fall back to another one.
export default defineConfig({
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  // Secrets live in src-tauri and are read by Rust. Nothing from .env is ever
  // inlined into the frontend bundle, so no VITE_/TAURI_ key prefixes here.
  envPrefix: [],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // .env holds backend credentials, not frontend config. Without this,
      // editing a key restarts the dev server and reloads the app.
      ignored: ["**/src-tauri/**", "**/.env", "**/.env.*"],
    },
  },
  build: {
    target: "es2022",
    sourcemap: process.env.NODE_ENV !== "production",
  },
});
