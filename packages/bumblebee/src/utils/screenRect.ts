import { Matrix, Vector3 } from "@babylonjs/core/Maths/math.vector";
import { Viewport } from "@babylonjs/core/Maths/math.viewport";
import type { AbstractMesh } from "@babylonjs/core/Meshes/abstractMesh";
import type { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import { resolveCanvasViewportSize } from "./screenToWorld";

export type ScreenRect = {
	left: number;
	right: number;
	top: number;
	bottom: number;
	width: number;
	height: number;
	centerX: number;
	centerY: number;
};

export type NodeLocalBounds = {
	minimum: Vector3;
	maximum: Vector3;
};

const rectFromPoints = (points: Vector3[]): ScreenRect | null => {
	const xs = points.map((point) => point.x);
	const ys = points.map((point) => point.y);
	const left = Math.min(...xs);
	const right = Math.max(...xs);
	const top = Math.min(...ys);
	const bottom = Math.max(...ys);
	if (![left, right, top, bottom].every(Number.isFinite)) return null;
	return {
		left,
		right,
		top,
		bottom,
		width: right - left,
		height: bottom - top,
		centerX: (left + right) / 2,
		centerY: (top + bottom) / 2
	};
};

const offsetScreenRect = (rect: ScreenRect | null, left: number, top: number): ScreenRect | null =>
	rect
		? {
				...rect,
				left: rect.left + left,
				right: rect.right + left,
				top: rect.top + top,
				bottom: rect.bottom + top,
				centerX: rect.centerX + left,
				centerY: rect.centerY + top
			}
		: null;

const boundsCorners = ({ minimum, maximum }: NodeLocalBounds) => [
	new Vector3(minimum.x, minimum.y, minimum.z),
	new Vector3(maximum.x, minimum.y, minimum.z),
	new Vector3(minimum.x, maximum.y, minimum.z),
	new Vector3(maximum.x, maximum.y, minimum.z),
	new Vector3(minimum.x, minimum.y, maximum.z),
	new Vector3(maximum.x, minimum.y, maximum.z),
	new Vector3(minimum.x, maximum.y, maximum.z),
	new Vector3(maximum.x, maximum.y, maximum.z)
];

const projectionContext = (node: TransformNode) => {
	const scene = node.getScene();
	const camera = scene.activeCamera;
	const engine = scene.getEngine();
	const canvas = engine.getRenderingCanvas();
	if (!camera || !canvas) return null;
	const { cssWidth, cssHeight } = resolveCanvasViewportSize(canvas, engine);
	if (!cssWidth || !cssHeight) return null;
	const canvasRect = canvas.getBoundingClientRect();
	return {
		viewport: new Viewport(0, 0, cssWidth, cssHeight),
		transform: scene.getTransformMatrix(),
		left: Number.isFinite(canvasRect.left) ? canvasRect.left : 0,
		top: Number.isFinite(canvasRect.top) ? canvasRect.top : 0
	};
};

export const captureNodeLocalBounds = (node: TransformNode): NodeLocalBounds | null => {
	node.computeWorldMatrix(true);
	const inverseRootWorld = node.getWorldMatrix().clone().invert();
	const localPoints: Vector3[] = [];
	for (const mesh of node.getChildMeshes(false) as AbstractMesh[]) {
		mesh.computeWorldMatrix(true);
		for (const point of mesh.getBoundingInfo().boundingBox.vectorsWorld) {
			localPoints.push(Vector3.TransformCoordinates(point, inverseRootWorld));
		}
	}
	if (localPoints.length === 0) return null;
	const xs = localPoints.map((point) => point.x);
	const ys = localPoints.map((point) => point.y);
	const zs = localPoints.map((point) => point.z);
	const minimum = new Vector3(Math.min(...xs), Math.min(...ys), Math.min(...zs));
	const maximum = new Vector3(Math.max(...xs), Math.max(...ys), Math.max(...zs));
	if (![...minimum.asArray(), ...maximum.asArray()].every(Number.isFinite)) return null;
	return { minimum, maximum };
};

export const projectNodeLocalBoundsToScreenRect = (
	node: TransformNode,
	bounds: NodeLocalBounds
): ScreenRect | null => {
	const context = projectionContext(node);
	if (!context) return null;
	node.computeWorldMatrix(true);
	const projected = boundsCorners(bounds).map((point) =>
		Vector3.Project(point, node.getWorldMatrix(), context.transform, context.viewport)
	);
	return offsetScreenRect(rectFromPoints(projected), context.left, context.top);
};

export const projectNodeToScreenRect = (node: TransformNode): ScreenRect | null => {
	const context = projectionContext(node);
	if (!context) return null;
	const meshes = node.getChildMeshes(false) as AbstractMesh[];
	const projected: Vector3[] = [];
	for (const mesh of meshes) {
		mesh.computeWorldMatrix(true);
		for (const point of mesh.getBoundingInfo().boundingBox.vectorsWorld) {
			projected.push(
				Vector3.Project(point, Matrix.Identity(), context.transform, context.viewport)
			);
		}
	}
	if (projected.length === 0) {
		node.computeWorldMatrix(true);
		projected.push(
			Vector3.Project(
				node.getAbsolutePosition(),
				Matrix.Identity(),
				context.transform,
				context.viewport
			)
		);
	}
	return offsetScreenRect(rectFromPoints(projected), context.left, context.top);
};
