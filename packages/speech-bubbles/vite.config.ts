import { defineConfig } from "vite";

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
				index: "src/index.ts",
				reveal: "src/reveal.ts",
				style: "src/style.ts"
			},
			formats: ["es"],
			fileName: (_format, entryName) => `${entryName}.js`,
			cssFileName: "style"
		},
		sourcemap: false,
		target: "es2022"
	}
});
