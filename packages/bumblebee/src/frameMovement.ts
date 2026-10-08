import { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import { Matrix, Quaternion, Vector3 } from "@babylonjs/core/Maths/math.vector";
import { createMoveToAnimation } from "./bumblebee/animations/moveTo";
import { FrameAnimationSampler } from "./frameAnimation";
import type { BumblebeeMoveAction, BumblebeeTimeline, BumblebeeTimelinePose } from "./timeline";

/** Compile the very same path/turn keys as live Actor.moveTo, once at readiness.
 * Sampling never starts an animation or draws new random numbers. The proxy is
 * a visual-center anchor, so turning the model cannot drag its screen position.
 */
export const createFrameMovement = (
	node: TransformNode,
	timeline: BumblebeeTimeline,
	worldCenter: (pose: BumblebeeTimelinePose, timeMs: number) => Vector3
) => {
	const proxy = new TransformNode(`${node.name}-frame-motion`, node.getScene());
	// The loader faces the camera from the origin. Resolve the authored starting
	// position here, just as moveTo faces the camera from its landing position.
	const camera = node.getScene().activeCamera!;
	const initialRotation = Quaternion.FromRotationMatrix(
		Matrix.LookAtLH(worldCenter(timeline.initialPose, 0), camera.position, camera.upVector).invert()
	);
	proxy.rotationQuaternion = initialRotation.clone();
	const moves: {
		action: BumblebeeMoveAction;
		group: ReturnType<typeof createMoveToAnimation>;
		sampler: FrameAnimationSampler;
	}[] = [];
	try {
		for (const action of timeline.actions) {
			if (action.type !== "moveTo") continue;
			const previous = moves.at(-1);
			if (previous && action.startMs < previous.action.startMs + previous.action.durationMs)
				throw new Error("Frame moveTo actions must not overlap on the same actor.");
			proxy.position.copyFrom(worldCenter(action.from, action.startMs));
			const group = createMoveToAnimation({
				node: proxy,
				destination: worldCenter(action.to, action.startMs),
				curveIntensity: action.curveIntensity ?? 0.5
			});
			// Explicit linear timing is supported without replacing the shared path.
			if (action.easing === "linear")
				for (const track of group.targetedAnimations) track.animation.setEasingFunction(null);
			const sampler = new FrameAnimationSampler([group]);
			moves.push({ action, group, sampler });
			sampler.apply(group.name, action.durationMs, { durationMs: action.durationMs });
		}
	} catch (error) {
		for (const move of moves) move.group.dispose();
		proxy.dispose();
		throw error;
	}
	return {
		seek(timeMs: number) {
			let move: (typeof moves)[number] | undefined;
			for (const candidate of moves) {
				if (candidate.action.startMs > timeMs) break;
				move = candidate;
			}
			if (!move) return { rotation: initialRotation.clone(), travelWeight: 0, elapsedMs: 0 };
			const { action, sampler, group } = move;
			const elapsedMs = Math.min(action.durationMs, timeMs - action.startMs);
			sampler.reset();
			sampler.apply(group.name, elapsedMs, { durationMs: action.durationMs });
			const rampMs = Math.min(160, action.durationMs / 4);
			const travelWeight = Math.max(
				0,
				Math.min(1, elapsedMs / rampMs, (action.durationMs - elapsedMs) / rampMs)
			);
			return {
				// At rest resolve the current stance's bounds/scale normally.
				center: elapsedMs < action.durationMs ? proxy.position.clone() : undefined,
				rotation: proxy.rotationQuaternion!.clone(),
				travelWeight,
				elapsedMs
			};
		},
		dispose() {
			for (const move of moves) move.group.dispose();
			proxy.dispose();
		}
	};
};
