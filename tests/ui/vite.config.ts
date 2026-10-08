import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";
export default defineConfig({
  optimizeDeps: { exclude: ["@babylonjs/core", "@babylonjs/loaders"] },
  root: fileURLToPath(new URL(".", import.meta.url)),
  publicDir: "../../public",
  plugins: [
    svelte(),
    {
      name: "fixture-silent-audio",
      configureServer(server) {
        server.middlewares.use((req, res, next) => {
          if (req.url !== "/media/fixture.wav") return next();
          const samples = 16000 * 6;
          const wav = Buffer.alloc(44 + samples * 2);
          wav.write("RIFF");
          wav.writeUInt32LE(wav.length - 8, 4);
          wav.write("WAVEfmt ", 8);
          wav.writeUInt32LE(16, 16);
          wav.writeUInt16LE(1, 20);
          wav.writeUInt16LE(1, 22);
          wav.writeUInt32LE(16000, 24);
          wav.writeUInt32LE(32000, 28);
          wav.writeUInt16LE(2, 32);
          wav.writeUInt16LE(16, 34);
          wav.write("data", 36);
          wav.writeUInt32LE(samples * 2, 40);
          res.setHeader("Content-Type", "audio/wav");
          res.end(wav);
        });
      },
    },
  ],
  resolve: { conditions: ["development", "module", "browser", "default"] },
  server: {
    host: "127.0.0.1",
    port: 1422,
    strictPort: true,
    fs: { allow: [fileURLToPath(new URL("../..", import.meta.url))] },
  },
});
