import { afterEach, describe, expect, test } from "bun:test";
import { Matrix, Vector2, Vector3 } from "@babylonjs/core/Maths/math.vector";
import type { Camera } from "@babylonjs/core/Cameras/camera";
import type { Scene } from "@babylonjs/core/scene";
import type { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import { resolveCanvasViewportSize, screenToWorld } from "./screenToWorld";

const originalWindow = globalThis.window;
const originalDocument = globalThis.document;
const originalHTMLElement = globalThis.HTMLElement;

const installViewportGlobals = (width: number, height: number) => {
	Object.assign(globalThis, {
		window: {
			innerWidth: width,
			innerHeight: height
		},
		document: {
			documentElement: {
				clientWidth: width,
				clientHeight: height
			}
		}
	});
};

const createCanvas = ({
	clientWidth,
	clientHeight,
	rectWidth,
	rectHeight,
	width = 0,
	height = 0
}: {
	clientWidth: number;
	clientHeight: number;
	rectWidth: number;
	rectHeight: number;
	width?: number;
	height?: number;
}) =>
	({
		clientWidth,
		clientHeight,
		width,
		height,
		getBoundingClientRect: () => ({
			x: 0,
			y: 0,
			top: 0,
			right: rectWidth,
			bottom: rectHeight,
			left: 0,
			width: rectWidth,
			height: rectHeight,
			toJSON: () => ({})
		})
	}) as HTMLCanvasElement;

const createEngine = (renderWidth: number, renderHeight: number) =>
	({
		getRenderWidth: () => renderWidth,
		getRenderHeight: () => renderHeight
	}) as ReturnType<Scene["getEngine"]>;

afterEach(() => {
	if (originalWindow === undefined) Reflect.deleteProperty(globalThis, "window");
	else globalThis.window = originalWindow;
	if (originalDocument === undefined) Reflect.deleteProperty(globalThis, "document");
	else globalThis.document = originalDocument;
	if (originalHTMLElement === undefined) Reflect.deleteProperty(globalThis, "HTMLElement");
	else globalThis.HTMLElement = originalHTMLElement;
});

describe("resolveCanvasViewportSize", () => {
	test("uses the canvas client size when available", () => {
		installViewportGlobals(1024, 768);
		const canvas = createCanvas({
			clientWidth: 640,
			clientHeight: 360,
			rectWidth: 640,
			rectHeight: 360
		});
		const engine = createEngine(1280, 720);

		expect(resolveCanvasViewportSize(canvas, engine)).toEqual({
			cssWidth: 640,
			cssHeight: 360,
			internalWidth: 1280,
			internalHeight: 720
		});
	});

	test("falls back to viewport dimensions when a hidden or restoring canvas reports zero size", () => {
		installViewportGlobals(1024, 768);
		const canvas = createCanvas({
			clientWidth: 0,
			clientHeight: 0,
			rectWidth: 0,
			rectHeight: 0
		});
		const engine = createEngine(0, 0);

		expect(resolveCanvasViewportSize(canvas, engine)).toEqual({
			cssWidth: 1024,
			cssHeight: 768,
			internalWidth: 1024,
			internalHeight: 768
		});
	});
});

describe("screenToWorld", () => {
	test("does not require a DOM HTMLElement global for Vector2 targets", () => {
		Reflect.deleteProperty(globalThis, "HTMLElement");
		installViewportGlobals(1024, 768);
		const canvas = createCanvas({
			clientWidth: 640,
			clientHeight: 360,
			rectWidth: 640,
			rectHeight: 360
		});
		const engine = createEngine(1280, 720);
		const scene = {
			getEngine: () => engine
		} as Scene;
		const camera = {
			getScene: () => scene,
			getViewMatrix: () => Matrix.Identity(),
			getProjectionMatrix: () => Matrix.Identity(),
			position: new Vector3(0, 0, 0),
			fov: Math.PI / 2
		} as unknown as Camera;
		const node = {
			metadata: {
				localDims: { w: 1, h: 1 }
			}
		} as TransformNode;

		const point = screenToWorld(node, new Vector2(320, 180), camera, canvas, 0.25);

		expect(Number.isFinite(point.x)).toBe(true);
		expect(Number.isFinite(point.y)).toBe(true);
		expect(Number.isFinite(point.z)).toBe(true);
	});

	test("normalizes non-positive scale and bounds inputs", () => {
		installViewportGlobals(1024, 768);
		const canvas = createCanvas({
			clientWidth: 640,
			clientHeight: 360,
			rectWidth: 640,
			rectHeight: 360
		});
		const engine = createEngine(1280, 720);
		const scene = {
			getEngine: () => engine
		} as Scene;
		const camera = {
			getScene: () => scene,
			getViewMatrix: () => Matrix.Identity(),
			getProjectionMatrix: () => Matrix.Identity(),
			position: new Vector3(0, 0, 0),
			fov: Math.PI / 2
		} as unknown as Camera;
		const node = {
			metadata: {
				localDims: { w: 1, h: 1 }
			}
		} as TransformNode;

		const point = screenToWorld(node, new Vector2(320, 180), camera, canvas, 0, 0, -10);

		expect(Number.isFinite(point.x)).toBe(true);
		expect(Number.isFinite(point.y)).toBe(true);
		expect(Number.isFinite(point.z)).toBe(true);
	});
});
