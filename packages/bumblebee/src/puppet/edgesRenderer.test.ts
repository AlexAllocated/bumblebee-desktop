import { describe, expect, test } from "bun:test";
import { NullEngine } from "@babylonjs/core/Engines/nullEngine";
import { Mesh } from "@babylonjs/core/Meshes/mesh";
import { Scene } from "@babylonjs/core/scene";
import "./index";

describe("puppet edge rendering", () => {
	test("loads Babylon's edge-renderer side effect before ghost mode disables edges", () => {
		const engine = new NullEngine();
		const scene = new Scene(engine);
		const mesh = new Mesh("ghost-puppet-part", scene);

		expect(() => mesh.disableEdgesRendering()).not.toThrow();

		scene.dispose();
		engine.dispose();
	});
});
