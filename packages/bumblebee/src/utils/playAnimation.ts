import { AnimationGroup } from "@babylonjs/core/Animations/animationGroup";
import { Animation } from "@babylonjs/core/Animations/animation";
import { Scene } from "@babylonjs/core/scene";
import type { Observer } from "@babylonjs/core/Misc/observable";
import z from "zod";
import { bbLog } from "./bbLog";

const OptionsSchema = z.object({
	animationGroup: z.instanceof(AnimationGroup),
	scene: z.instanceof(Scene),
	signal: z.instanceof(AbortSignal).optional(),
	blendMode: z.enum(["none", "blend", "additive"]).default("blend"),
	duration: z.number().optional(),
	loop: z.boolean().default(false),
	speed: z.number().default(1),
	weight: z.number().default(1),
	from: z.number().optional(),
	to: z.number().optional(),
	goToFrame: z.number().optional(),
	blendingSpeed: z.number().default(0.05),
	strictEnd: z.boolean().default(false),
	resetToInitialOnStop: z.boolean().default(false)
});

type Options = z.input<typeof OptionsSchema>;

const activeAnimationRuns = new WeakMap<AnimationGroup, symbol>();
const activeBlendAnimations = new WeakMap<
	AnimationGroup,
	ReturnType<Scene["beginDirectAnimation"]>
>();

const animateGroupWeight = (
	scene: Scene,
	animationGroup: AnimationGroup,
	toWeight: number,
	speed = 8
) => {
	const animation = new Animation(
		toWeight > animationGroup.weight ? "blendWeightIn" : "blendWeightOut",
		"weight",
		60,
		Animation.ANIMATIONTYPE_FLOAT,
		Animation.ANIMATIONLOOPMODE_CONSTANT
	);
	animation.setKeys([
		{ frame: 0, value: animationGroup.weight },
		{ frame: 60, value: toWeight }
	]);
	return scene.beginDirectAnimation(animationGroup, [animation], 0, 60, false, speed);
};

const playAnimation = async (options: Options) => {
	const opts = OptionsSchema.parse(options);

	const { scene, animationGroup, signal } = opts;
	if (signal?.aborted) throw new Error("Aborted");
	if (opts.loop && opts.duration === undefined && !signal) {
		throw new Error("Looped animations require a duration or abort signal");
	}
	const runToken = Symbol(animationGroup.name);
	activeAnimationRuns.set(animationGroup, runToken);
	activeBlendAnimations.get(animationGroup)?.stop();
	activeBlendAnimations.delete(animationGroup);

	switch (opts.blendMode) {
		case "none":
			animationGroup.enableBlending = false;
			animationGroup.isAdditive = false;
			break;
		case "blend":
			animationGroup.enableBlending = true;
			animationGroup.isAdditive = false;
			break;
		case "additive":
			animationGroup.enableBlending = false;
			if (!animationGroup.isAdditive) {
				AnimationGroup.MakeAnimationAdditive(animationGroup);
			}
			break;
	}

	if (opts.from !== undefined) animationGroup.from = opts.from;
	if (opts.to !== undefined) animationGroup.to = opts.to;
	if (opts.goToFrame !== undefined) animationGroup.goToFrame(opts.goToFrame);
	if (opts.weight !== undefined) animationGroup.weight = opts.weight;
	animationGroup.speedRatio = opts.speed;
	animationGroup.blendingSpeed = opts.blendingSpeed;

	if (opts.blendMode === "additive") {
		animationGroup.weight = 0;
		animationGroup.play(opts.loop);
		const animatable = animateGroupWeight(scene, animationGroup, opts.weight);
		activeBlendAnimations.set(animationGroup, animatable);
		animatable.onAnimationEndObservable.addOnce(() => {
			if (activeBlendAnimations.get(animationGroup) === animatable) {
				activeBlendAnimations.delete(animationGroup);
			}
		});
	} else {
		animationGroup.play(opts.loop);
	}

	bbLog("playAnim", "start", {
		name: animationGroup.name,
		from: animationGroup.from,
		to: animationGroup.to,
		loop: opts.loop,
		speed: opts.speed,
		weight: opts.weight,
		mode: opts.blendMode
	});

	const stopForAbort = () => {
		if (opts.blendMode === "additive") {
			const animatable = animateGroupWeight(scene, animationGroup, 0);
			activeBlendAnimations.set(animationGroup, animatable);
			animatable.onAnimationEndObservable.addOnce(() => {
				if (activeAnimationRuns.get(animationGroup) === runToken) {
					animationGroup.stop(opts.resetToInitialOnStop);
					animationGroup.weight = opts.weight;
				}
				if (activeBlendAnimations.get(animationGroup) === animatable) {
					activeBlendAnimations.delete(animationGroup);
				}
			});
		} else {
			animationGroup.stop(opts.resetToInitialOnStop);
		}
	};

	await new Promise<void>((resolve, reject) => {
		let resolved = false;
		let durationTimer: ReturnType<typeof setTimeout> | null = null;
		let endObserver: Observer<AnimationGroup> | null = null;

		const settle = (callback: () => void) => {
			if (resolved) return;
			resolved = true;
			if (durationTimer !== null) clearTimeout(durationTimer);
			durationTimer = null;
			if (endObserver) animationGroup.onAnimationGroupEndObservable.remove(endObserver);
			endObserver = null;
			signal?.removeEventListener("abort", onAbort);
			callback();
		};

		const onAbort = () => {
			stopForAbort();
			settle(() => reject(new Error("Aborted")));
		};

		if (!opts.loop) {
			endObserver = animationGroup.onAnimationGroupEndObservable.add(() => {
				settle(() => {
					// If strictEnd, ensure the group is exactly at its 'to' frame before resolving
					if (opts.strictEnd && typeof animationGroup.to === "number") {
						try {
							animationGroup.goToFrame(animationGroup.to);
						} catch {}
					}
					resolve();
				});
			});
		}
		signal?.addEventListener("abort", onAbort, { once: true });
		if (opts.duration) {
			durationTimer = setTimeout(() => {
				settle(() => resolve());
			}, opts.duration);
		}
	});

	// Do not reset to initial values; leave properties at final values
	if (activeAnimationRuns.get(animationGroup) === runToken) {
		animationGroup.stop(opts.resetToInitialOnStop);
	}
	bbLog("playAnim", "end", { name: animationGroup.name });
};

const playDisposableAnimation = async (options: Options) => {
	try {
		await playAnimation(options);
	} finally {
		options.animationGroup.dispose();
	}
};

export { playAnimation, playDisposableAnimation };
