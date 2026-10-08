import { defineConfig } from "vite";

const external = [
	"@babylonjs/core",
	"@babylonjs/loaders",
	"@hivetech/speech-bubbles",
	"immer",
	"marching-squares",
	"uuid",
	"xstate",
	"zod"
];

export default defineConfig({
	plugins: [
		{
			name: "css-type-export",
			generateBundle() {
				this.emitFile({ type: "asset", fileName: "style.css.d.ts", source: "export {};\n" });
			}
		}
	],
	build: {
		lib: {
			entry: {
				"frame-renderer": "src/frameRenderer.ts",
				bitcrusher: "src/utils/bitcrusher.ts",
				"dialogue-bleeps": "src/overlay/dialogueBleeps.ts",
				"handle-colors": "src/utils/handleColors.ts",
				index: "src/index.ts",
				"low-level": "src/low-level.ts",
				"screen-to-world": "src/utils/screenToWorld.ts",
				"speech-assets": "src/speech-assets.ts",
				timeline: "src/timeline.ts",
				tools: "src/tools.ts"
			},
			formats: ["es"],
			fileName: (_format, entryName) => `${entryName}.js`,
			cssFileName: "style"
		},
		sourcemap: false,
		target: "es2022",
		rollupOptions: {
			external: (id) => external.some((pkg) => id === pkg || id.startsWith(`${pkg}/`))
		}
	}
});
