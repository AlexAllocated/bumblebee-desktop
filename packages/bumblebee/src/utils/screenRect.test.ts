import { afterEach, describe, expect, test } from "bun:test";
import { FreeCamera } from "@babylonjs/core/Cameras/freeCamera";
import { NullEngine } from "@babylonjs/core/Engines/nullEngine";
import { Vector3 } from "@babylonjs/core/Maths/math.vector";
import { MeshBuilder } from "@babylonjs/core/Meshes/meshBuilder";
import { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import { Scene } from "@babylonjs/core/scene";
import { captureNodeLocalBounds, projectNodeLocalBoundsToScreenRect } from "./screenRect";

const engines: NullEngine[] = [];

afterEach(() => {
	for (const engine of engines.splice(0)) engine.dispose();
});

describe("stable node screen bounds", () => {
	test("ignore animated child movement while following the actor root", () => {
		const engine = new NullEngine({
			renderWidth: 640,
			renderHeight: 360,
			textureSize: 512,
			deterministicLockstep: false,
			lockstepMaxSteps: 4
		});
		engines.push(engine);
		Object.defineProperty(engine, "getRenderingCanvas", {
			value: () => ({
				clientWidth: 640,
				clientHeight: 360,
				width: 640,
				height: 360,
				getBoundingClientRect: () => ({ width: 640, height: 360 })
			})
		});
		const scene = new Scene(engine);
		const camera = new FreeCamera("camera", new Vector3(0, 0, -10), scene);
		camera.setTarget(Vector3.Zero());
		scene.activeCamera = camera;
		const root = new TransformNode("actor-root", scene);
		const animatedChild = MeshBuilder.CreateBox("animated-child", { size: 2 }, scene);
		animatedChild.parent = root;
		scene.render();

		const stableBounds = captureNodeLocalBounds(root);
		expect(stableBounds).not.toBeNull();
		const initial = projectNodeLocalBoundsToScreenRect(root, stableBounds!);
		expect(initial).not.toBeNull();

		animatedChild.position.x = 5;
		scene.render();
		const afterChildAnimation = projectNodeLocalBoundsToScreenRect(root, stableBounds!);
		expect(afterChildAnimation).toEqual(initial);

		root.position.x = 1;
		scene.render();
		const afterRootMovement = projectNodeLocalBoundsToScreenRect(root, stableBounds!);
		expect(afterRootMovement?.centerX).not.toBe(initial?.centerX);
	});

	test("reports viewport coordinates when the render canvas is mounted away from the origin", () => {
		const engine = new NullEngine({
			renderWidth: 640,
			renderHeight: 360,
			textureSize: 512,
			deterministicLockstep: false,
			lockstepMaxSteps: 4
		});
		engines.push(engine);
		Object.defineProperty(engine, "getRenderingCanvas", {
			value: () => ({
				clientWidth: 640,
				clientHeight: 360,
				width: 640,
				height: 360,
				getBoundingClientRect: () => ({ left: 240, top: 120, width: 640, height: 360 })
			})
		});
		const scene = new Scene(engine);
		const camera = new FreeCamera("camera", new Vector3(0, 0, -10), scene);
		camera.setTarget(Vector3.Zero());
		scene.activeCamera = camera;
		const root = new TransformNode("actor-root", scene);
		const child = MeshBuilder.CreateBox("child", { size: 2 }, scene);
		child.parent = root;
		scene.render();

		const bounds = captureNodeLocalBounds(root);
		const rect = bounds ? projectNodeLocalBoundsToScreenRect(root, bounds) : null;

		expect(rect?.centerX).toBeCloseTo(560);
		expect(rect?.centerY).toBeCloseTo(300);
	});
});
