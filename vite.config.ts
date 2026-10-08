import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  base: "./",
  resolve: { conditions: ["development", "module", "browser", "default"] },
  // Keep Babylon loader/shader registrations in one module graph in development.
  optimizeDeps: { exclude: ["@babylonjs/core", "@babylonjs/loaders"] },
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
