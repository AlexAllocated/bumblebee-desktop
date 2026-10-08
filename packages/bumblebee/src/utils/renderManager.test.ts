import { afterEach, describe, expect, test } from "bun:test";
import type { Engine } from "@babylonjs/core/Engines/engine";
import type { Scene } from "@babylonjs/core/scene";
import { dispose, disposeSceneRegistration, registerScene, unregisterScene } from "./renderManager";

const originalWindow = globalThis.window;
const originalDocument = globalThis.document;
const originalResizeObserver = globalThis.ResizeObserver;
const originalRequestAnimationFrame = globalThis.requestAnimationFrame;
const originalCancelAnimationFrame = globalThis.cancelAnimationFrame;

type ListenerTarget = {
	readonly adds: Record<string, number>;
	readonly removes: Record<string, number>;
	addEventListener(type: string, listener: EventListenerOrEventListenerObject): void;
	removeEventListener(type: string, listener: EventListenerOrEventListenerObject): void;
};

const createListenerTarget = (): ListenerTarget => ({
	adds: {},
	removes: {},
	addEventListener(type) {
		this.adds[type] = (this.adds[type] ?? 0) + 1;
	},
	removeEventListener(type) {
		this.removes[type] = (this.removes[type] ?? 0) + 1;
	}
});

const installRenderManagerHarness = () => {
	let nextFrameId = 1;
	let nextFrame: FrameRequestCallback | null = null;
	let documentHidden = false;
	let resizeObserverDisconnects = 0;
	const windowTarget = createListenerTarget();
	const documentTarget = createListenerTarget();
	const documentElement = {};

	class FakeResizeObserver {
		observe() {}
		disconnect() {
			resizeObserverDisconnects += 1;
		}
	}

	Object.assign(globalThis, {
		window: {
			...windowTarget,
			location: { hostname: "example.test" },
			devicePixelRatio: 2
		},
		document: {
			...documentTarget,
			documentElement,
			get hidden() {
				return documentHidden;
			}
		},
		ResizeObserver: FakeResizeObserver,
		requestAnimationFrame: (callback: FrameRequestCallback) => {
			nextFrame = callback;
			return nextFrameId++;
		},
		cancelAnimationFrame: () => {}
	});

	return {
		windowTarget,
		documentTarget,
		setDocumentHidden(hidden: boolean) {
			documentHidden = hidden;
		},
		runNextFrame(time: number) {
			const callback = nextFrame;
			nextFrame = null;
			callback?.(time);
		},
		get resizeObserverDisconnects() {
			return resizeObserverDisconnects;
		}
	};
};

const createFakeScene = () => {
	const scene = {
		disposeCalls: 0,
		renderCalls: 0,
		isDisposed: false,
		performancePriority: 0,
		skipPointerMovePicking: false,
		render() {
			this.renderCalls += 1;
		},
		dispose() {
			this.disposeCalls += 1;
			this.isDisposed = true;
		}
	};
	return scene as unknown as Scene & {
		disposeCalls: number;
		isDisposed: boolean;
		renderCalls: number;
	};
};

const createFakeEngine = (canvas: HTMLCanvasElement) => {
	const engine = {
		disposeCalls: 0,
		hardwareScalingLevels: [] as number[],
		maxFPS: 60,
		setHardwareScalingLevel(level: number) {
			this.hardwareScalingLevels.push(level);
		},
		resize() {},
		getRenderingCanvas: () => canvas,
		beginFrame() {},
		endFrame() {},
		dispose() {
			this.disposeCalls += 1;
		}
	};
	return engine as unknown as Engine & {
		disposeCalls: number;
		hardwareScalingLevels: number[];
	};
};

const createFakeCanvas = () => {
	let removeCalls = 0;
	const parentElement = {
		removeChild() {
			removeCalls += 1;
		}
	};
	const canvas = {
		isConnected: true,
		clientWidth: 640,
		clientHeight: 360,
		parentElement,
		getBoundingClientRect: () => ({ width: 640, height: 360 })
	} as unknown as HTMLCanvasElement;
	return {
		canvas,
		get removeCalls() {
			return removeCalls;
		}
	};
};

afterEach(() => {
	dispose();
	Object.assign(globalThis, {
		window: originalWindow,
		document: originalDocument,
		ResizeObserver: originalResizeObserver,
		requestAnimationFrame: originalRequestAnimationFrame,
		cancelAnimationFrame: originalCancelAnimationFrame
	});
});

describe("render manager scene registration", () => {
	test("cleans old resize listeners when the same scene id is registered again", () => {
		const harness = installRenderManagerHarness();
		const { canvas } = createFakeCanvas();
		const scene = createFakeScene();
		const engine = createFakeEngine(canvas);

		registerScene("same-scene", scene, engine, canvas);
		registerScene("same-scene", scene, engine, canvas);

		expect(harness.windowTarget.adds.resize).toBe(2);
		expect(harness.windowTarget.removes.resize).toBe(1);
		expect(harness.documentTarget.adds.visibilitychange).toBe(2);
		expect(harness.documentTarget.removes.visibilitychange).toBe(1);
		expect(harness.resizeObserverDisconnects).toBe(1);

		expect(unregisterScene("same-scene", scene)).toBe(true);
		expect(harness.windowTarget.removes.resize).toBe(2);
		expect(harness.documentTarget.removes.visibilitychange).toBe(2);
		expect(harness.resizeObserverDisconnects).toBe(2);
		expect(scene.disposeCalls).toBe(0);
		expect(engine.disposeCalls).toBe(0);
	});

	test("disposes registered scene resources through the full disposal path", () => {
		const harness = installRenderManagerHarness();
		const canvasHandle = createFakeCanvas();
		const scene = createFakeScene();
		const engine = createFakeEngine(canvasHandle.canvas);

		registerScene("full-dispose", scene, engine, canvasHandle.canvas);

		expect(disposeSceneRegistration("full-dispose", scene)).toBe(true);
		expect(scene.disposeCalls).toBe(1);
		expect(engine.disposeCalls).toBe(1);
		expect(canvasHandle.removeCalls).toBe(1);
		expect(harness.windowTarget.removes.resize).toBe(1);
		expect(harness.documentTarget.removes.visibilitychange).toBe(1);
		expect(harness.resizeObserverDisconnects).toBe(1);

		expect(disposeSceneRegistration("full-dispose", scene)).toBe(false);
		expect(scene.disposeCalls).toBe(1);
		expect(engine.disposeCalls).toBe(1);
		expect(canvasHandle.removeCalls).toBe(1);
	});

	test("releases the engine only after the scene disposal callback finishes", async () => {
		installRenderManagerHarness();
		const canvasHandle = createFakeCanvas();
		const scene = createFakeScene();
		const engine = createFakeEngine(canvasHandle.canvas);

		registerScene("scene-hook", scene, engine, canvasHandle.canvas);

		expect(disposeSceneRegistration("scene-hook", scene, { disposeScene: false })).toBe(true);
		expect(scene.disposeCalls).toBe(0);
		expect(engine.disposeCalls).toBe(0);
		await Promise.resolve();
		expect(engine.disposeCalls).toBe(1);
		expect(canvasHandle.removeCalls).toBe(1);
	});

	test("global dispose releases registered scenes and engines", () => {
		installRenderManagerHarness();
		const first = createFakeCanvas();
		const second = createFakeCanvas();
		const firstScene = createFakeScene();
		const secondScene = createFakeScene();
		const firstEngine = createFakeEngine(first.canvas);
		const secondEngine = createFakeEngine(second.canvas);

		registerScene("first", firstScene, firstEngine, first.canvas);
		registerScene("second", secondScene, secondEngine, second.canvas);

		dispose();

		expect(firstScene.disposeCalls).toBe(1);
		expect(secondScene.disposeCalls).toBe(1);
		expect(firstEngine.disposeCalls).toBe(1);
		expect(secondEngine.disposeCalls).toBe(1);
		expect(first.removeCalls).toBe(1);
		expect(second.removeCalls).toBe(1);
	});

	test("does not render scenes while the page is hidden", () => {
		const harness = installRenderManagerHarness();
		harness.setDocumentHidden(true);
		const { canvas } = createFakeCanvas();
		const scene = createFakeScene();
		const engine = createFakeEngine(canvas);

		registerScene("hidden-scene", scene, engine, canvas);
		harness.runNextFrame(performance.now() + 100);

		expect(scene.renderCalls).toBe(0);
	});

	test("honors a per-scene render pixel ratio cap", () => {
		installRenderManagerHarness();
		const { canvas } = createFakeCanvas();
		const scene = createFakeScene();
		const engine = createFakeEngine(canvas);

		registerScene("capped-scene", scene, engine, canvas, 1, "normal", {
			maxRenderPixelRatio: 1.25
		});

		expect(engine.hardwareScalingLevels.at(-1)).toBeCloseTo(0.8);
	});

	test("skips a suspended scene and resumes it without re-registering", () => {
		const harness = installRenderManagerHarness();
		const { canvas } = createFakeCanvas();
		const scene = createFakeScene();
		const engine = createFakeEngine(canvas);
		let active = false;

		registerScene("suspendable-scene", scene, engine, canvas, 1, "normal", {
			shouldRender: () => active
		});
		harness.runNextFrame(performance.now() + 100);
		expect(scene.renderCalls).toBe(0);

		active = true;
		harness.runNextFrame(performance.now() + 200);
		expect(scene.renderCalls).toBe(1);
	});

	test("never changes a scene's selected profile after registration", () => {
		const harness = installRenderManagerHarness();
		const { canvas } = createFakeCanvas();
		const scene = createFakeScene();
		const engine = createFakeEngine(canvas);

		registerScene("fixed-profile", scene, engine, canvas, 1, "normal");
		const startedAt = performance.now();
		for (let index = 1; index <= 10; index += 1) {
			harness.runNextFrame(startedAt + index * 250);
		}

		expect(engine.hardwareScalingLevels).toHaveLength(1);
		expect(scene.skipPointerMovePicking).toBe(true);
	});
});
