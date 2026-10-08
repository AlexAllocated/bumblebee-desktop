import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath, URL } from "node:url";

export default defineConfig({
  plugins: [svelte()],
  base: "./",
  resolve: {
    alias: {
      "@speech-bubbles": fileURLToPath(
        new URL("./src/renderer/speech-bubbles/index.ts", import.meta.url),
      ),
    },
  },
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: {
      ignored: [
        "**/target/**",
        "**/src-tauri/**",
        "**/crates/**",
        "**/vendor/**",
      ],
    },
  },
  build: {
    target: "es2022",
    rollupOptions: { input: { app: "index.html", overlay: "overlay.html" } },
  },
});
