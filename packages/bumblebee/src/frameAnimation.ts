import type { AnimationGroup } from "@babylonjs/core/Animations/animationGroup";
import { Quaternion, Vector3 } from "@babylonjs/core/Maths/math.vector";

type Value = number | Quaternion | Vector3;
const copy = (value: Value): Value => (typeof value === "number" ? value : value.clone());
type Binding = { target: Record<string, Value>; property: string; initial: Value };

/** Uses Babylon's public keyframe evaluator, never starts an Animatable or a timer. */
export class FrameAnimationSampler {
	#bindings: Binding[] = [];
	constructor(readonly groups: AnimationGroup[]) {
		for (const group of groups)
			for (const track of group.targetedAnimations) {
				const path = track.animation.targetProperty.split(".");
				let target = track.target;
				for (const part of path.slice(0, -1)) target = target[part];
				const property = path[path.length - 1];
				const initial = target[property];
				if (
					typeof initial !== "number" &&
					!(initial instanceof Quaternion) &&
					!(initial instanceof Vector3)
				) {
					throw new Error(
						`Unsupported frame animation property: ${track.animation.targetProperty}`
					);
				}
				if (
					!this.#bindings.some(
						(binding) => binding.target === target && binding.property === property
					)
				) {
					this.#bindings.push({ target, property, initial: copy(initial) });
				}
			}
	}
	reset() {
		for (const binding of this.#bindings) binding.target[binding.property] = copy(binding.initial);
	}
	apply(
		name: string,
		elapsedMs: number,
		options: { loop?: boolean; weight?: number; durationMs?: number } = {}
	) {
		const group = this.groups.find((candidate) => candidate.name === name);
		if (!group) throw new Error(`Missing frame animation: ${name}`);
		const weight = Math.max(0, Math.min(1, options.weight ?? 1));
		if (!weight) return;
		for (const track of group.targetedAnimations) {
			const animation = track.animation;
			const span = group.to - group.from;
			const elapsedFrames = options.durationMs
				? (Math.max(0, elapsedMs) / options.durationMs) * span
				: (Math.max(0, elapsedMs) / 1000) * animation.framePerSecond;
			const frame =
				group.from +
				(options.loop && span > 0 ? elapsedFrames % span : Math.min(span, elapsedFrames));
			const value = animation.evaluate(frame) as Value;
			const path = animation.targetProperty.split(".");
			let target = track.target;
			for (const part of path.slice(0, -1)) target = target[part];
			const key = path[path.length - 1];
			const base = target[key] as Value;
			if (!group.isAdditive) {
				if (value instanceof Quaternion && base instanceof Quaternion)
					target[key] = Quaternion.Slerp(base, value, weight);
				else if (value instanceof Vector3 && base instanceof Vector3)
					target[key] = Vector3.Lerp(base, value, weight);
				else if (typeof value === "number" && typeof base === "number")
					target[key] = base + (value - base) * weight;
				else target[key] = copy(value);
			} else if (value instanceof Quaternion && base instanceof Quaternion) {
				target[key] = base.multiply(Quaternion.Slerp(Quaternion.Identity(), value, weight));
			} else if (value instanceof Vector3 && base instanceof Vector3) {
				target[key] = base.add(value.scale(weight));
			} else if (typeof value === "number" && typeof base === "number")
				target[key] = base + value * weight;
		}
	}
}
