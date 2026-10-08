import { resolveCanvasViewportSize, screenToWorld } from "../../utils/screenToWorld";
import { Animation } from "@babylonjs/core/Animations/animation";
import { AnimationGroup } from "@babylonjs/core/Animations/animationGroup";
import { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import { Vector2, Vector3, Quaternion, Matrix } from "@babylonjs/core/Maths/math.vector";
import { Viewport } from "@babylonjs/core/Maths/math.viewport";
import { CubicEase, EasingFunction } from "@babylonjs/core/Animations/easing";
import { Camera } from "@babylonjs/core/Cameras/camera";
import z from "zod";
import { sceneRandom } from "../../utils/sceneRandom";

const HTMLElementSchema = z.custom<HTMLElement>(
	(value) => typeof HTMLElement !== "undefined" && value instanceof HTMLElement,
	"Expected an HTMLElement"
);

const OptionsSchema = z.object({
	node: z.instanceof(TransformNode),
	destination: HTMLElementSchema.or(z.instanceof(Vector2)).or(z.instanceof(Vector3)),
	scalePercentage: z.number().optional().default(1),
	boundsWidth: z.number().optional(),
	boundsHeight: z.number().optional(),
	curveIntensity: z.number().optional().default(0.5)
});
type Options = z.input<typeof OptionsSchema>;

const PathOptionsSchema = z.object({
	node: z.instanceof(TransformNode),
	destinations: z
		.array(HTMLElementSchema.or(z.instanceof(Vector2)).or(z.instanceof(Vector3)))
		.min(1),
	startScalePercentage: z.number().optional(),
	scalePercentage: z.number().optional().default(1),
	boundsWidth: z.number().optional(),
	boundsHeight: z.number().optional()
});
type PathOptions = z.input<typeof PathOptionsSchema>;

const isFiniteVector3 = (vector: Vector3) =>
	Number.isFinite(vector.x) && Number.isFinite(vector.y) && Number.isFinite(vector.z);

const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));

const safeNormalize = (vector: Vector3, fallback: Vector3) => {
	if (!isFiniteVector3(vector) || vector.lengthSquared() < 1e-12) return fallback.clone();
	const normalized = vector.normalize();
	return isFiniteVector3(normalized) ? normalized : fallback.clone();
};

const quaternionDot = (a: Quaternion, b: Quaternion) =>
	a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;

const alignQuaternionSign = (quaternion: Quaternion, reference: Quaternion) =>
	quaternionDot(quaternion, reference) < 0
		? new Quaternion(-quaternion.x, -quaternion.y, -quaternion.z, -quaternion.w)
		: quaternion;

const catmullRom = (p0: Vector3, p1: Vector3, p2: Vector3, p3: Vector3, t: number) => {
	const t2 = t * t;
	const t3 = t2 * t;
	return p1
		.scale(2)
		.add(p2.subtract(p0).scale(t))
		.add(p0.scale(2).subtract(p1.scale(5)).add(p2.scale(4)).subtract(p3).scale(t2))
		.add(
			p3
				.subtract(p0)
				.add(p1.scale(3).subtract(p2.scale(3)))
				.scale(t3)
		)
		.scale(0.5);
};

const createMovementGroup = ({
	name,
	node,
	posKeys,
	targetPosition
}: {
	name: string;
	node: TransformNode;
	posKeys: { frame: number; value: Vector3 }[];
	targetPosition: Vector3;
}) => {
	const scene = node.getScene();
	const camera: Camera = scene.activeCamera!;
	const frames = posKeys.at(-1)?.frame ?? 0;
	const firstPosition = posKeys[0]?.value ?? node.position;
	const direction = safeNormalize(targetPosition.subtract(firstPosition), Vector3.Forward());

	const rotKeys: { frame: number; value: Quaternion }[] = [];
	const initialQuat = node.rotationQuaternion
		? node.rotationQuaternion.clone()
		: Quaternion.FromEulerAngles(node.rotation.x, node.rotation.y, node.rotation.z);
	node.rotationQuaternion = initialQuat.clone();

	const camQuat = Quaternion.FromRotationMatrix(
		Matrix.LookAtLH(targetPosition, camera.position, camera.upVector).invert()
	);

	const startBlend = Math.min(12, Math.max(4, Math.round(frames * 0.14)));
	const endBlend = Math.min(12, Math.max(4, Math.round(frames * 0.14)));
	const tangentWindow = Math.min(8, Math.max(2, Math.round(posKeys.length * 0.06)));
	let previousRotQuat = initialQuat;
	for (let index = 0; index < posKeys.length; index += 1) {
		const key = posKeys[index]!;
		const pCurr = key.value;
		const pBefore = posKeys[Math.max(index - tangentWindow, 0)]?.value ?? pCurr;
		const pAfter = posKeys[Math.min(index + tangentWindow, posKeys.length - 1)]?.value ?? pCurr;
		const pNext = posKeys[Math.min(index + 1, posKeys.length - 1)]?.value ?? pCurr;
		const tangent =
			pAfter.subtract(pBefore).lengthSquared() > 1e-10
				? pAfter.subtract(pBefore)
				: pNext.subtract(pCurr);
		const tangDir = safeNormalize(tangent, direction);
		const pathQuat = Quaternion.FromRotationMatrix(
			Matrix.LookAtLH(pCurr, pCurr.add(tangDir), camera.upVector).invert()
		);

		let keyQuat: Quaternion;
		if (key.frame <= startBlend) {
			const t = key.frame / Math.max(1, startBlend);
			keyQuat = Quaternion.Slerp(initialQuat, alignQuaternionSign(pathQuat, initialQuat), t);
		} else if (key.frame >= frames - endBlend) {
			const t = (key.frame - (frames - endBlend)) / Math.max(1, endBlend);
			keyQuat = Quaternion.Slerp(pathQuat, alignQuaternionSign(camQuat, pathQuat), t);
		} else {
			keyQuat = pathQuat;
		}
		keyQuat = alignQuaternionSign(keyQuat, previousRotQuat);
		previousRotQuat = keyQuat;
		rotKeys.push({ frame: key.frame, value: keyQuat });
	}

	const group = new AnimationGroup(name, scene);
	const posAnim = new Animation(
		"position",
		"position",
		60,
		Animation.ANIMATIONTYPE_VECTOR3,
		Animation.ANIMATIONLOOPMODE_CONSTANT
	);
	posAnim.setKeys(posKeys);
	const ease = new CubicEase();
	ease.setEasingMode(EasingFunction.EASINGMODE_EASEINOUT);
	posAnim.setEasingFunction(ease);
	group.addTargetedAnimation(posAnim, node);

	const rotAnim = new Animation(
		"rotationQuaternion",
		"rotationQuaternion",
		60,
		Animation.ANIMATIONTYPE_QUATERNION,
		Animation.ANIMATIONLOOPMODE_CONSTANT
	);
	rotAnim.setKeys(rotKeys);
	rotAnim.setEasingFunction(ease);
	group.addTargetedAnimation(rotAnim, node);

	return group;
};

/**
 * Moves a TransformNode along a curved or straight path to a destination,
 * adjusting depth so the node maintains a consistent visual size.
 * Supports HTMLElement, Vector2, or Vector3 destinations. Accounts for device
 * pixel ratio via adaptToDeviceRatio.
 *
 * @param node             The TransformNode (parent of mesh) to animate
 * @param destination      Target: HTMLElement center, Vector2 screen point, or Vector3 world point
 * @param scalePercentage  Fraction of screen height the object should fill
 * @param boundsWidth      Optional custom width in px for size reference
 * @param boundsHeight     Optional custom height in px for size reference
 * @param frames           Total animation frames (default 60)
 * @param curveIntensity   0 for straight; >0 to add random curve (0.1–1.0)
 * @returns                The AnimationGroup controlling the motion
 */
export function createMoveToAnimation(options: Options): AnimationGroup {
	const { node, destination, scalePercentage, boundsWidth, boundsHeight, curveIntensity } =
		OptionsSchema.parse(options);
	const frames = 60;
	const scene = node.getScene();
	const engine = scene.getEngine();
	const camera: Camera = scene.activeCamera!;
	const canvas = engine.getRenderingCanvas()!;

	const viewMatrix = camera.getViewMatrix(true);
	const projMatrix = camera.getProjectionMatrix(true);
	const transformMatrix = scene.getTransformMatrix();
	let targetPosition: Vector3;
	if (destination instanceof Vector3) {
		targetPosition = destination;
	} else {
		targetPosition = screenToWorld(
			node,
			destination,
			camera,
			canvas,
			scalePercentage,
			boundsWidth,
			boundsHeight
		);
	}
	if (!isFiniteVector3(targetPosition)) {
		targetPosition = node.position.clone();
	}
	const direction = safeNormalize(targetPosition.subtract(camera.position), Vector3.Forward());

	// 2. Generate full posKeys for frames
	const posKeys: { frame: number; value: Vector3 }[] = [];
	const startPos = isFiniteVector3(node.position) ? node.position.clone() : targetPosition.clone();
	const { cssWidth, cssHeight, internalWidth, internalHeight } = resolveCanvasViewportSize(
		canvas,
		engine
	);
	const viewport = new Viewport(0, 0, cssWidth, cssHeight);
	const projectedStart = Vector3.Project(startPos, Matrix.Identity(), transformMatrix, viewport);
	const projectedTarget = Vector3.Project(
		targetPosition,
		Matrix.Identity(),
		transformMatrix,
		viewport
	);
	const screenDistance =
		isFiniteVector3(projectedStart) && isFiniteVector3(projectedTarget)
			? Vector2.Distance(
					new Vector2(projectedStart.x, projectedStart.y),
					new Vector2(projectedTarget.x, projectedTarget.y)
				)
			: Number.POSITIVE_INFINITY;
	const effectiveCurveIntensity = curveIntensity * clamp((screenDistance - 420) / 680, 0, 1);
	let control: Vector3;
	if (effectiveCurveIntensity > 0) {
		const mid = startPos.add(targetPosition).scale(0.5);
		const perp = safeNormalize(camera.upVector.cross(direction), camera.upVector);
		const mag = Vector3.Distance(startPos, targetPosition) * effectiveCurveIntensity;
		const offset = perp
			.scale((sceneRandom(scene) * 2 - 1) * mag)
			.add(camera.upVector.scale((sceneRandom(scene) * 2 - 1) * mag));
		const rawControl = mid.add(offset);

		// clamp control in screen and unproject
		const projCtrl = Vector3.Project(rawControl, Matrix.Identity(), transformMatrix, viewport);
		if (isFiniteVector3(projCtrl)) {
			const clampedX = Math.min(Math.max(projCtrl.x, 0), cssWidth);
			const clampedY = Math.min(Math.max(projCtrl.y, 0), cssHeight);
			control = Vector3.Unproject(
				new Vector3(
					clampedX * (internalWidth / cssWidth),
					clampedY * (internalHeight / cssHeight),
					projCtrl.z
				),
				internalWidth,
				internalHeight,
				Matrix.Identity(),
				viewMatrix,
				projMatrix
			);
			if (!isFiniteVector3(control)) {
				control = mid;
			}
		} else {
			control = mid;
		}

		for (let i = 0; i <= frames; i++) {
			const t = i / frames;
			const p0 = startPos.scale((1 - t) * (1 - t));
			const p1 = control.scale(2 * (1 - t) * t);
			const p2 = targetPosition.scale(t * t);
			posKeys.push({ frame: i, value: p0.add(p1).add(p2) });
		}
	} else {
		// straight interpolation
		for (let i = 0; i <= frames; i++) {
			const t = i / frames;
			const pos = Vector3.Lerp(startPos, targetPosition, t);
			posKeys.push({ frame: i, value: pos });
		}
	}

	return createMovementGroup({ name: "moveToAnimationGroup", node, posKeys, targetPosition });
}

export function createMoveAlongAnimation(options: PathOptions): AnimationGroup {
	const { node, destinations, startScalePercentage, scalePercentage, boundsWidth, boundsHeight } =
		PathOptionsSchema.parse(options);
	const scene = node.getScene();
	const camera: Camera = scene.activeCamera!;
	const canvas = scene.getEngine().getRenderingCanvas()!;
	const startPos = isFiniteVector3(node.position) ? node.position.clone() : Vector3.Zero();
	const scaleStart = startScalePercentage ?? scalePercentage;
	const targetPositions = destinations.map((destination, index) => {
		if (destination instanceof Vector3) return destination;
		const progress = (index + 1) / destinations.length;
		const waypointScale = scaleStart + (scalePercentage - scaleStart) * progress;
		return screenToWorld(
			node,
			destination,
			camera,
			canvas,
			waypointScale,
			boundsWidth,
			boundsHeight
		);
	});
	const points = [startPos, ...targetPositions].filter(isFiniteVector3);
	const targetPosition = points.at(-1) ?? startPos;
	if (points.length < 2) {
		return createMovementGroup({
			name: "moveAlongAnimationGroup",
			node,
			posKeys: [{ frame: 0, value: startPos }],
			targetPosition
		});
	}

	const framesPerSegment = 34;
	const posKeys: { frame: number; value: Vector3 }[] = [];
	for (let segment = 0; segment < points.length - 1; segment += 1) {
		const p0 = points[Math.max(0, segment - 1)]!;
		const p1 = points[segment]!;
		const p2 = points[segment + 1]!;
		const p3 = points[Math.min(points.length - 1, segment + 2)]!;
		for (let frame = 0; frame < framesPerSegment; frame += 1) {
			const t = frame / framesPerSegment;
			posKeys.push({
				frame: segment * framesPerSegment + frame,
				value: catmullRom(p0, p1, p2, p3, t)
			});
		}
	}
	posKeys.push({ frame: (points.length - 1) * framesPerSegment, value: targetPosition });

	return createMovementGroup({ name: "moveAlongAnimationGroup", node, posKeys, targetPosition });
}
