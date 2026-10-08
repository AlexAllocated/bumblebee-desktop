import { Color4 } from "@babylonjs/core/Maths/math.color";
import { FreeCamera } from "@babylonjs/core/Cameras/freeCamera";
import { HemisphericLight } from "@babylonjs/core/Lights/hemisphericLight";
import { Scene } from "@babylonjs/core/scene";
import { Vector3 } from "@babylonjs/core/Maths/math.vector";
import {
	createBabylonEngine,
	resolveBabylonPerformanceProfile,
	resolveInitialBabylonPerformanceProfile,
	type BabylonCanvasResizeOptions,
	type BabylonPerformanceProfileName
} from "../utils/babylonPerformance";
import { disposeSceneRegistration, registerScene } from "../utils/renderManager";

const mountOverlayCanvas = (
	canvas: HTMLCanvasElement,
	container: HTMLElement | null | undefined,
	zIndex: number
) => {
	const target = container ?? document.body;
	const scopedToContainer = target !== document.body;
	canvas.classList.toggle("absolute", scopedToContainer);
	canvas.classList.toggle("fixed", !scopedToContainer);
	canvas.classList.toggle("h-full", scopedToContainer);
	canvas.classList.toggle("h-screen", !scopedToContainer);
	canvas.classList.toggle("w-full", scopedToContainer);
	canvas.classList.toggle("w-screen", !scopedToContainer);
	canvas.style.position = scopedToContainer ? "absolute" : "fixed";
	canvas.style.inset = "0";
	canvas.style.width = scopedToContainer ? "100%" : "100vw";
	canvas.style.height = scopedToContainer ? "100%" : "100vh";
	canvas.style.zIndex = String(zIndex);
	target.appendChild(canvas);
};

const createScene = (options?: {
	container?: HTMLElement | null;
	id?: string;
	hidden?: boolean;
	zIndex?: number;
	performanceProfile?: BabylonPerformanceProfileName;
	canvasResizeOptions?: BabylonCanvasResizeOptions;
	shouldRender?: () => boolean;
	/** Offline callers own resize, animation sampling and rendering. */
	manualRender?: boolean;
}) => {
	const sceneId = options?.id ?? "bumblebee-overlay";
	const requestedPerformanceProfile = options?.performanceProfile
		? resolveBabylonPerformanceProfile(options.performanceProfile)
		: resolveBabylonPerformanceProfile();
	const initialPerformanceProfile = options?.performanceProfile
		? requestedPerformanceProfile
		: resolveInitialBabylonPerformanceProfile();
	const canvas = document.createElement("canvas");
	canvas.classList.add("bumblebee-overlay-canvas");
	canvas.classList.add("inset-0", "pointer-events-none", "touch-none");
	canvas.style.display = "block";
	canvas.style.pointerEvents = "none";
	canvas.style.touchAction = "none";
	canvas.style.border = "0";
	canvas.style.margin = "0";
	canvas.style.padding = "0";
	if (options?.hidden) {
		canvas.style.display = "none";
		canvas.style.opacity = "0";
	}
	mountOverlayCanvas(canvas, options?.container, options?.zIndex ?? 2147482990);
	let engine: ReturnType<typeof createBabylonEngine>;
	try {
		engine = createBabylonEngine(canvas, initialPerformanceProfile.name, {}, true);
	} catch (error) {
		canvas.remove();
		throw error;
	}
	const scene = new Scene(engine);
	scene.clearColor = new Color4(0, 0, 0, 0);
	const leftLight = new HemisphericLight("leftLight", new Vector3(-1, 1, -1), scene);
	leftLight.intensity = 0.6;
	const rightLight = new HemisphericLight("rightLight", new Vector3(1, 1, -1), scene);
	rightLight.intensity = 0.6;
	const camera = new FreeCamera("camera", new Vector3(0, 0, -1), scene);
	camera.fov = 0.25;
	camera.minZ = 0.01;

	scene.onDisposeObservable.addOnce(() => {
		disposeSceneRegistration(sceneId, scene, { disposeScene: false });
	});

	// Register with render manager instead of running individual render loop
	if (!options?.manualRender)
		registerScene(sceneId, scene, engine, canvas, 0, initialPerformanceProfile.name, {
			...options?.canvasResizeOptions,
			shouldRender: options?.shouldRender
		}); // Priority 0 (highest)

	return scene;
};

export { createScene, mountOverlayCanvas };
