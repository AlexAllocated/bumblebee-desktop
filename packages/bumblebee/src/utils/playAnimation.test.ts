import { afterEach, describe, expect, test } from "bun:test";
import { AnimationGroup } from "@babylonjs/core/Animations/animationGroup";
import { NullEngine } from "@babylonjs/core/Engines/nullEngine";
import { Scene } from "@babylonjs/core/scene";
import { playAnimation, playDisposableAnimation } from "./playAnimation";

const engines: NullEngine[] = [];

const createAnimationGroup = () => {
	const engine = new NullEngine();
	engines.push(engine);
	const scene = new Scene(engine);
	return {
		scene,
		animationGroup: new AnimationGroup("loop", scene)
	};
};

afterEach(() => {
	while (engines.length > 0) {
		engines.pop()?.dispose();
	}
});

describe("playAnimation", () => {
	test("rejects looped animations that have no settlement condition", async () => {
		const { scene, animationGroup } = createAnimationGroup();

		await expect(playAnimation({ scene, animationGroup, loop: true })).rejects.toThrow(
			"Looped animations require a duration or abort signal"
		);
	});

	test("disposes one-shot groups after successful playback", async () => {
		const { scene, animationGroup } = createAnimationGroup();

		await playDisposableAnimation({
			animationGroup,
			scene,
			loop: true,
			duration: 1,
			blendMode: "none"
		});

		expect(scene.animationGroups).not.toContain(animationGroup);
	});

	test("disposes one-shot groups when playback fails", async () => {
		const { scene, animationGroup } = createAnimationGroup();
		const abort = new AbortController();
		abort.abort();

		await expect(
			playDisposableAnimation({ animationGroup, scene, signal: abort.signal })
		).rejects.toThrow("Aborted");
		expect(scene.animationGroups).not.toContain(animationGroup);
	});
});
