import type { Animation } from "@babylonjs/core/Animations/animation";
import type { AnimationGroup } from "@babylonjs/core/Animations/animationGroup";
import { describe, expect, test } from "bun:test";
import { pruneBumblebeeFacialAnimationTargets } from "./loadModel";

const animationGroup = (
	name: string,
	targets: Array<string | { name: string; className: string }>
) => {
	const removed: string[] = [];
	const targetedAnimations = targets.map((target) => {
		const targetName = typeof target === "string" ? target : target.name;
		return {
			target: {
				name: targetName,
				getClassName: () => (typeof target === "string" ? "TransformNode" : target.className)
			},
			animation: { name: `${targetName}-animation` } as Animation
		};
	});
	return {
		group: {
			name,
			targetedAnimations,
			removeTargetedAnimation: (animation: Animation) => removed.push(animation.name)
		} as unknown as AnimationGroup,
		removed
	};
};

describe("Bumblebee facial animation targets", () => {
	test("keeps permanent facial emotes from overriding wings and walking bones", () => {
		const { group, removed } = animationGroup("mad", [
			"eyesDriver",
			"mouthDriver",
			"colorDriver",
			"tennaL01",
			"tennaR01",
			{ name: "eyesMad", className: "MorphTarget" },
			"wingL",
			"wingR",
			"hips",
			"legFL"
		]);

		pruneBumblebeeFacialAnimationTargets(group);

		expect(removed).toEqual([
			"wingL-animation",
			"wingR-animation",
			"hips-animation",
			"legFL-animation"
		]);
	});

	test("does not mask full-body movement animations", () => {
		const { group, removed } = animationGroup("walking", ["wingL", "wingR", "hips"]);
		pruneBumblebeeFacialAnimationTargets(group);
		expect(removed).toEqual([]);
	});
});
