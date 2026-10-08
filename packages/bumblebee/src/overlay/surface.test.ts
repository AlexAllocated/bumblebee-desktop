import { afterEach, describe, expect, mock, test } from "bun:test";
import type { Scene } from "@babylonjs/core/scene";

afterEach(() => {
	mock.restore();
});

describe("Surface", () => {
	test("creates scenes inside the configured overlay container", async () => {
		const canvas = {} as HTMLCanvasElement;
		const resize = mock(() => {});
		const render = mock(() => {});
		const scene = {
			isDisposed: false,
			dispose: mock(() => {}),
			render,
			getEngine: () => ({ getRenderingCanvas: () => canvas, resize })
		} as unknown as Scene;
		const createScene = mock((_options: unknown) => scene);
		const mountOverlayCanvas = mock(() => {});
		const container = {} as HTMLElement;

		mock.module("../bumblebee/createScene.ts", () => ({ createScene, mountOverlayCanvas }));

		const { Surface } = await import(`./surface.ts?surface=${Date.now()}-${Math.random()}`);
		const surface = new Surface({ surface: { container }, zIndex: 4321 });

		expect(createScene).toHaveBeenCalledTimes(1);
		expect(createScene.mock.calls[0]?.[0]).toMatchObject({
			container,
			zIndex: 4321
		});

		const crtViewport = {} as HTMLElement;
		expect(surface.setContainer(crtViewport)).toBe(canvas);
		expect(mountOverlayCanvas).toHaveBeenCalledWith(canvas, crtViewport, 4321);
		expect(resize).toHaveBeenCalledTimes(1);
		expect(render).toHaveBeenCalledTimes(1);

		surface.dispose();
		expect(scene.dispose).toHaveBeenCalledTimes(1);
	});

	test("repeatedly mounts and unmounts without double-disposing scenes", async () => {
		const scenes = Array.from({ length: 3 }, () => {
			const canvas = {} as HTMLCanvasElement;
			return {
				isDisposed: false,
				dispose: mock(() => {}),
				render: mock(() => {}),
				getEngine: () => ({
					getRenderingCanvas: () => canvas,
					resize: mock(() => {})
				})
			} as unknown as Scene;
		});
		let sceneIndex = 0;
		const createScene = mock(() => scenes[sceneIndex++]!);

		mock.module("../bumblebee/createScene.ts", () => ({
			createScene,
			mountOverlayCanvas: mock(() => {})
		}));

		const { Surface } = await import(`./surface.ts?cycles=${Date.now()}-${Math.random()}`);
		for (const _scene of scenes) {
			const surface = new Surface({});
			surface.dispose();
			surface.dispose();
		}

		expect(createScene).toHaveBeenCalledTimes(3);
		for (const scene of scenes) {
			expect(scene.dispose).toHaveBeenCalledTimes(1);
		}
	});
});
