import { describe, expect, test } from "bun:test";
import { NullEngine } from "@babylonjs/core/Engines/nullEngine";
import { Scene } from "@babylonjs/core/scene";
import { Animation } from "@babylonjs/core/Animations/animation";
import { AnimationGroup } from "@babylonjs/core/Animations/animationGroup";
import { FrameAnimationSampler } from "./frameAnimation";

describe("frame animation sampling", () => {
	test("random access restores pose without creating live animatables", () => {
		const engine = new NullEngine();
		const scene = new Scene(engine);
		const target = { amount: 3 };
		const group = new AnimationGroup("pose", scene);
		const track = new Animation("amount", "amount", 30, Animation.ANIMATIONTYPE_FLOAT);
		track.setKeys([
			{ frame: 0, value: 0 },
			{ frame: 30, value: 10 }
		]);
		group.addTargetedAnimation(track, target);
		const sampler = new FrameAnimationSampler([group]);
		for (const time of [750, 200, 750, 0, 200]) {
			sampler.reset();
			sampler.apply("pose", time);
			expect(target.amount).toBeCloseTo(time / 100);
		}
		sampler.reset();
		expect(target.amount).toBe(3);
		sampler.apply("pose", 500, { weight: 0.5 });
		expect(target.amount).toBe(4);
		expect(scene.animatables).toHaveLength(0);
		scene.dispose();
		engine.dispose();
	});
	test("additive layers do not accumulate across repeated seeks", () => {
		const engine = new NullEngine();
		const scene = new Scene(engine);
		const target = { amount: 3 };
		const group = new AnimationGroup("talk", scene);
		group.isAdditive = true;
		const track = new Animation("amount", "amount", 30, Animation.ANIMATIONTYPE_FLOAT);
		track.setKeys([
			{ frame: 0, value: 0 },
			{ frame: 30, value: 10 }
		]);
		group.addTargetedAnimation(track, target);
		const sampler = new FrameAnimationSampler([group]);
		for (let i = 0; i < 3; i++) {
			sampler.reset();
			sampler.apply("talk", 500, { weight: 0.5 });
			expect(target.amount).toBe(5.5);
		}
		scene.dispose();
		engine.dispose();
	});
});
