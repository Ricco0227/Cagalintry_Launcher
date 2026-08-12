import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

// `tauri dev` sets this when serving to a device on the LAN.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react(), tailwindcss()],

  // Tauri's own output is the interesting part of the terminal; don't wipe it.
  clearScreen: false,

  resolve: {
    alias: { "@": path.resolve(import.meta.dirname, "src") },
  },

  server: {
    port: 1420,
    // A silent port change would leave Tauri pointing at nothing.
    strictPort: true,
    host: host || false,
    // Spread rather than `hmr: undefined` — with exactOptionalPropertyTypes an
    // explicit undefined is not the same as an absent key.
    ...(host ? { hmr: { protocol: "ws", host, port: 1421 } } : {}),
    watch: {
      // Rust rebuilds are Cargo's job; watching these would loop.
      ignored: ["**/src-tauri/**"],
    },
  },

  envPrefix: ["VITE_", "TAURI_ENV_*"],

  build: {
    // The only browser this ships to is the bundled WebView2 / WKWebView, so
    // there is no reason to down-level modern syntax.
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome110" : "safari15",
    // Vite 8 minifies with oxc by default; `true` picks whatever it ships with.
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
