import { expect, test } from "bun:test";
import { NullEngine } from "@babylonjs/core/Engines/nullEngine";
import { Scene } from "@babylonjs/core/scene";
import { FreeCamera } from "@babylonjs/core/Cameras/freeCamera";
import { Quaternion, Vector3 } from "@babylonjs/core/Maths/math.vector";
import { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import { createFrameMovement } from "./frameMovement";
import { createBumblebeeTimelineBuilder } from "./timeline";
import { setSceneRandomSeed } from "./utils/sceneRandom";

test("frame moveTo uses live turning/travel keys and is deterministic without animatables", () => {
	const engine = new NullEngine();
	engine.getRenderingCanvas = () =>
		({
			clientWidth: 1080,
			clientHeight: 1920,
			width: 1080,
			height: 1920,
			getBoundingClientRect: () => ({ width: 1080, height: 1920 })
		}) as HTMLCanvasElement;
	const scene = new Scene(engine);
	const camera = new FreeCamera("camera", new Vector3(0, 0, -20), scene);
	scene.activeCamera = camera;
	camera.setTarget(Vector3.Zero());
	scene.render();
	const node = new TransformNode("bee", scene);
	node.rotationQuaternion = Quaternion.FromEulerAngles(0, Math.PI, 0);
	const timeline = createBumblebeeTimelineBuilder({ initialPose: { position: { x: -1, y: 0 } } })
		.stance("flying")
		.moveTo({ position: { x: 1, y: 0 } }, { startMs: 200, durationMs: 1600, curveIntensity: 0.5 })
		.moveTo({ position: { x: -1, y: 0 } }, { startMs: 2300, durationMs: 1600 })
		.build();
	const compile = () => {
		setSceneRandomSeed(scene, 321);
		return createFrameMovement(
			node,
			timeline,
			(pose) => new Vector3(pose.position.x * 12, pose.position.y * 12, 0)
		);
	};
	const movement = compile();
	const initial = movement.seek(0);
	const facing = Vector3.Forward().applyRotationQuaternion(initial.rotation);
	const towardCamera = camera.position.subtract(new Vector3(-12, 0, 0)).normalize();
	expect(Vector3.Dot(facing, towardCamera)).toBeCloseTo(1, 6);
	expect(Math.abs(Quaternion.Dot(initial.rotation, movement.seek(3900).rotation))).toBeCloseTo(
		1,
		6
	);
	const mid = movement.seek(1000);
	expect(mid.travelWeight).toBe(1);
	expect(Math.abs(Quaternion.Dot(initial.rotation, mid.rotation))).toBeLessThan(0.99);
	for (const t of [1790, 201, 1600, 0, 1000]) movement.seek(t);
	expect(movement.seek(1000)).toEqual(mid);
	expect(movement.seek(200).center).toEqual(new Vector3(-12, 0, 0));
	expect(movement.seek(1800).travelWeight).toBe(0);
	expect(movement.seek(1800).center).toBeUndefined();
	const fresh = compile();
	expect(fresh.seek(1000)).toEqual(mid);
	expect(scene.animatables).toHaveLength(0);
	movement.dispose();
	fresh.dispose();
	expect(scene.animationGroups).toHaveLength(0);
	scene.dispose();
	engine.dispose();
});
