// utils/screenToWorld.ts
import { Vector3, Matrix, Vector2 } from "@babylonjs/core/Maths/math.vector";
import { Camera } from "@babylonjs/core/Cameras/camera";
import { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import type { Scene } from "@babylonjs/core/scene";
import { bbLog } from "./bbLog";

export type CanvasViewportSize = {
	cssWidth: number;
	cssHeight: number;
	internalWidth: number;
	internalHeight: number;
};

type RenderSizeSource = ReturnType<Scene["getEngine"]>;
type LocalDims = {
	w: number;
	h: number;
};

const finitePositive = (value: unknown): value is number =>
	typeof value === "number" && Number.isFinite(value) && value > 0;

const firstFinitePositive = (...values: unknown[]) => {
	for (const value of values) {
		if (finitePositive(value)) return value;
	}
	return 1;
};

const fallbackViewportSize = () => ({
	width: firstFinitePositive(
		typeof window !== "undefined" ? window.innerWidth : undefined,
		typeof document !== "undefined" ? document.documentElement?.clientWidth : undefined
	),
	height: firstFinitePositive(
		typeof window !== "undefined" ? window.innerHeight : undefined,
		typeof document !== "undefined" ? document.documentElement?.clientHeight : undefined
	)
});

const isHTMLElementTarget = (target: HTMLElement | Vector2): target is HTMLElement =>
	typeof HTMLElement !== "undefined" && target instanceof HTMLElement;

const resolveNodeLocalDims = (node: TransformNode): LocalDims => {
	const metadataDims = node.metadata?.localDims as { w?: unknown; h?: unknown } | undefined;
	if (finitePositive(metadataDims?.w) && finitePositive(metadataDims?.h)) {
		return { w: metadataDims.w, h: metadataDims.h };
	}
	try {
		const bounds = node.getHierarchyBoundingVectors(true);
		const width = bounds.max.x - bounds.min.x;
		const height = bounds.max.y - bounds.min.y;
		if (finitePositive(width) && finitePositive(height)) {
			return { w: width, h: height };
		}
	} catch {
		// Some intermediate puppet roots have no ready hierarchy during resize or reload.
	}
	return { w: 1, h: 1 };
};

export function resolveCanvasViewportSize(
	canvas: HTMLCanvasElement,
	engine: RenderSizeSource
): CanvasViewportSize {
	const rect = canvas.getBoundingClientRect();
	const fallback = fallbackViewportSize();
	const cssWidth = firstFinitePositive(canvas.clientWidth, rect.width, fallback.width);
	const cssHeight = firstFinitePositive(canvas.clientHeight, rect.height, fallback.height);
	const internalWidth = firstFinitePositive(engine.getRenderWidth(), cssWidth, canvas.width);
	const internalHeight = firstFinitePositive(engine.getRenderHeight(), cssHeight, canvas.height);

	return { cssWidth, cssHeight, internalWidth, internalHeight };
}

/**
 * Given an element and a scene/camera, compute the world‐space
 * point at which a unit‐scaled object would appear to be centered
 * on that element, at the correct distance so it fills
 * `scalePercentage` of the screen.
 */
export function screenToWorld(
	node: TransformNode,
	target: HTMLElement | Vector2,
	camera: Camera,
	canvas: HTMLCanvasElement,
	scalePercentage: number,
	boundsWidth?: number,
	boundsHeight?: number,
	fitMode: "vertical" | "max" = "max"
): Vector3 {
	const scene = camera.getScene()!;
	const engine = scene.getEngine();
	const {
		cssWidth,
		cssHeight,
		internalWidth: internalW,
		internalHeight: internalH
	} = resolveCanvasViewportSize(canvas, engine);

	let xScreen: number | null = null;
	let yScreen: number | null = null;
	if (isHTMLElementTarget(target)) {
		// 1) element center in CSS pixels (relative to canvas)
		const crect = canvas.getBoundingClientRect();
		const drect = target.getBoundingClientRect();
		xScreen = (drect.left + drect.right) / 2 - crect.left;
		yScreen = (drect.top + drect.bottom) / 2 - crect.top;
	} else {
		xScreen = target.x;
		yScreen = target.y;
	}

	// 2) convert to internal render pixels
	const xInt = xScreen * (internalW / cssWidth);
	const yInt = yScreen * (internalH / cssHeight);

	// 3) unproject near & far to get a direction vector
	const view = camera.getViewMatrix(true);
	const proj = camera.getProjectionMatrix(true);
	const world = Matrix.Identity() as Matrix;
	const near = Vector3.Unproject(
		new Vector3(xInt, yInt, 0),
		internalW,
		internalH,
		world,
		view,
		proj
	);
	const far = Vector3.Unproject(
		new Vector3(xInt, yInt, 1),
		internalW,
		internalH,
		world,
		view,
		proj
	);
	const direction = far.subtract(near).normalize();

	// 4) compute the distance so object fills the right fraction of the screen
	const { w: objW, h: objH } = resolveNodeLocalDims(node);
	const safeScalePercentage = finitePositive(scalePercentage)
		? Math.max(0.02, scalePercentage)
		: 0.02;
	const refW = finitePositive(boundsWidth) ? boundsWidth : cssWidth;
	const refH = finitePositive(boundsHeight) ? boundsHeight : cssHeight;
	const fracH = safeScalePercentage * (refH / cssHeight);
	const fracW = safeScalePercentage * (refW / cssWidth);
	const vFOV = camera.fov;
	const tan2 = Math.tan(vFOV / 2);
	const aspect = internalW / internalH;
	const dV = objH / (2 * fracH * tan2);
	const dH = objW / (2 * fracW * tan2 * aspect);
	const dist = fitMode === "vertical" ? dV : Math.max(dV, dH);

	const out = camera.position.add(direction.scale(dist));
	bbLog("screenToWorld", "calc", {
		fitMode,
		scalePercentage: safeScalePercentage,
		objW,
		objH,
		internalW,
		internalH,
		xInt: Math.round(xInt),
		yInt: Math.round(yInt),
		dV: +dV.toFixed(3),
		dH: +dH.toFixed(3),
		dist: +dist.toFixed(3),
		outY: +out.y.toFixed(3)
	});
	return out;
}
