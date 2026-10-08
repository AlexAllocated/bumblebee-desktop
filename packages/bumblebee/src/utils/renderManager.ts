import { Engine } from "@babylonjs/core/Engines/engine";
import { Scene } from "@babylonjs/core/scene";
import { PerformanceMonitor as BabylonPerformanceMonitor } from "@babylonjs/core/Misc/performanceMonitor";
import {
	applyBabylonPerformanceProfile,
	isBabylonProfileLearningEnabled,
	recordBabylonPerformanceObservation,
	resolveBabylonPerformanceProfile,
	resolveHardwareScalingLevel,
	type BabylonCanvasResizeOptions,
	type BabylonPerformanceProfileName
} from "./babylonPerformance";
import {
	startFrame,
	endFrame,
	startPerformanceMonitoring,
	stopPerformanceMonitoring,
	type PerformanceStats
} from "./performanceMonitor";

export type RenderSceneOptions = BabylonCanvasResizeOptions & {
	shouldRender?: () => boolean;
};

interface RenderableScene {
	scene: Scene;
	engine: Engine;
	canvas: HTMLCanvasElement;
	priority: number; // Lower number = higher priority
	performanceProfile: BabylonPerformanceProfileName;
	renderOptions: RenderSceneOptions;
	cleanup: () => void;
}

// Module-level state
const scenes = new Map<string, RenderableScene>();
const observationMonitor = new BabylonPerformanceMonitor(60);
let isRunning = false;
let animationFrame: number | undefined;
let lastTime = 0;
let nextObservationTime = 0;

const PERFORMANCE_OBSERVATION_INTERVAL_MS = 1000;

function getCurrentTargetFPS(): number {
	if (scenes.size === 0) {
		return resolveBabylonPerformanceProfile("normal").targetFPS;
	}
	return Math.min(
		...Array.from(scenes.values()).map(
			(renderable) => resolveBabylonPerformanceProfile(renderable.performanceProfile).targetFPS
		)
	);
}

function getCurrentPerformanceProfile(): BabylonPerformanceProfileName {
	return Array.from(scenes.values()).some(
		(renderable) => renderable.performanceProfile === "degraded"
	)
		? "degraded"
		: "normal";
}

function canvasHasUsableSize(canvas: HTMLCanvasElement): boolean {
	const rect = canvas.getBoundingClientRect();
	return (canvas.clientWidth > 0 && canvas.clientHeight > 0) || (rect.width > 0 && rect.height > 0);
}

function applyProfileToScene(
	renderable: RenderableScene,
	performanceProfile: BabylonPerformanceProfileName
): void {
	const profile = applyBabylonPerformanceProfile(
		renderable.engine,
		renderable.scene,
		performanceProfile
	);
	renderable.performanceProfile = profile.name;
	renderable.engine.setHardwareScalingLevel(
		resolveHardwareScalingLevel(profile.name, undefined, renderable.renderOptions)
	);
	renderable.engine.resize();
}

function resetPerformanceObservation(currentTime = performance.now()): void {
	observationMonitor.reset();
	nextObservationTime = currentTime + PERFORMANCE_OBSERVATION_INTERVAL_MS;
}

function updatePerformanceObservation(currentTime: number, renderedScene: boolean): void {
	if (!isBabylonProfileLearningEnabled() || scenes.size === 0 || !renderedScene) return;
	observationMonitor.sampleFrame(currentTime);
	if (!observationMonitor.isSaturated || currentTime < nextObservationTime) return;

	nextObservationTime = currentTime + PERFORMANCE_OBSERVATION_INTERVAL_MS;
	const measuredFPS = observationMonitor.averageFPS;
	if (!Number.isFinite(measuredFPS) || measuredFPS <= 0) return;
	recordBabylonPerformanceObservation(
		getCurrentPerformanceProfile(),
		measuredFPS,
		getCurrentTargetFPS()
	);
}

function startRenderLoop(): void {
	if (isRunning) return;

	isRunning = true;
	lastTime = performance.now();
	resetPerformanceObservation(lastTime);

	// Start performance monitoring in development mode
	if (typeof window !== "undefined" && window.location?.hostname === "localhost") {
		startPerformanceMonitoring((stats: PerformanceStats) => {
			// Log performance stats in development
			if (stats.fps < 20) {
				console.warn(
					"Low FPS detected:",
					stats.fps,
					"Memory:",
					Math.round(stats.memoryUsed / 1024 / 1024) + "MB"
				);
			}
		});
	}

	renderLoop();
}

function stopRenderLoop(): void {
	isRunning = false;
	if (animationFrame) {
		cancelAnimationFrame(animationFrame);
	}
	resetPerformanceObservation();
	stopPerformanceMonitoring();
}

function disposeRegisteredScene(
	renderable: RenderableScene,
	options: { disposeScene?: boolean } = {}
): void {
	const disposeScene = options.disposeScene ?? true;
	renderable.cleanup();
	if (disposeScene && !renderable.scene.isDisposed) {
		try {
			renderable.scene.dispose();
		} catch (error) {
			console.warn("Failed to dispose registered scene:", error);
		}
	}
	const releaseRenderer = () => {
		try {
			renderable.engine.dispose();
		} catch (error) {
			console.warn("Failed to dispose registered engine:", error);
		}
		if (renderable.canvas.parentElement) {
			renderable.canvas.parentElement.removeChild(renderable.canvas);
		}
	};
	// Scene.onDisposeObservable fires before Babylon finishes accessing its engine.
	// Detach scheduling immediately, then release the engine after synchronous teardown.
	if (disposeScene) releaseRenderer();
	else queueMicrotask(releaseRenderer);
}

function renderLoop(currentTime?: number): void {
	if (!isRunning) return;

	currentTime = currentTime || performance.now();
	if (typeof document !== "undefined" && document.hidden) {
		lastTime = currentTime;
		resetPerformanceObservation(currentTime);
		animationFrame = requestAnimationFrame(renderLoop);
		return;
	}
	const deltaTime = currentTime - lastTime;
	const targetFPS = getCurrentTargetFPS();
	const frameInterval = 1000 / targetFPS;

	// Frame rate limiting
	if (deltaTime >= frameInterval) {
		startFrame(); // Start performance monitoring for this frame
		let renderedScene = false;

		// Sort scenes by priority and render
		const sortedScenes = Array.from(scenes.values()).sort((a, b) => a.priority - b.priority);

		for (const { scene, engine, canvas, renderOptions } of sortedScenes) {
			try {
				if (renderOptions.shouldRender?.() === false) continue;
				// Only render if the canvas is still connected to DOM
				if (engine.getRenderingCanvas()?.isConnected && canvasHasUsableSize(canvas)) {
					engine.beginFrame();
					scene.render();
					engine.endFrame();
					renderedScene = true;
				}
			} catch (error) {
				console.warn("Render error in scene:", error);
			}
		}

		endFrame(); // End performance monitoring for this frame
		updatePerformanceObservation(currentTime, renderedScene);
		lastTime = currentTime - (deltaTime % frameInterval);
	}

	animationFrame = requestAnimationFrame(renderLoop);
}

// Public API
export function registerScene(
	id: string,
	scene: Scene,
	engine: Engine,
	canvas: HTMLCanvasElement,
	priority = 1,
	performanceProfile: BabylonPerformanceProfileName = resolveBabylonPerformanceProfile(null).name,
	renderOptions: RenderSceneOptions = {}
): void {
	let resizeFrame: number | null = null;
	let retryTimer: ReturnType<typeof setTimeout> | null = null;
	let resizeObserver: ResizeObserver | null = null;

	const clearRetry = () => {
		if (!retryTimer) return;
		clearTimeout(retryTimer);
		retryTimer = null;
	};

	const runResize = (attempt = 0) => {
		resizeFrame = null;
		if (!canvas.isConnected) return;

		if (!canvasHasUsableSize(canvas)) {
			if (!retryTimer && attempt < 10) {
				retryTimer = setTimeout(() => {
					retryTimer = null;
					scheduleResize(attempt + 1);
				}, 100);
			}
			return;
		}

		clearRetry();
		engine.resize();
	};

	const scheduleResize = (attempt = 0) => {
		if (resizeFrame !== null) return;
		resizeFrame = requestAnimationFrame(() => runResize(attempt));
	};

	const handleResize = () => scheduleResize();
	window.addEventListener("resize", handleResize);
	window.addEventListener("focus", handleResize);
	window.addEventListener("pageshow", handleResize);
	document.addEventListener("visibilitychange", handleResize);

	if (typeof ResizeObserver !== "undefined") {
		resizeObserver = new ResizeObserver(() => scheduleResize());
		resizeObserver.observe(canvas);
		if (canvas.parentElement) resizeObserver.observe(canvas.parentElement);
		resizeObserver.observe(document.documentElement);
	}

	scheduleResize();

	// Keep cleanup local to the render manager (no object pollution)
	const cleanup = () => {
		if (resizeFrame !== null) {
			cancelAnimationFrame(resizeFrame);
			resizeFrame = null;
		}
		clearRetry();
		resizeObserver?.disconnect();
		window.removeEventListener("resize", handleResize);
		window.removeEventListener("focus", handleResize);
		window.removeEventListener("pageshow", handleResize);
		document.removeEventListener("visibilitychange", handleResize);
	};

	const existing = scenes.get(id);
	if (existing) {
		if (existing.scene !== scene) {
			scenes.delete(id);
			disposeRegisteredScene(existing);
		} else {
			existing.cleanup();
		}
	}

	scenes.set(id, {
		scene,
		engine,
		canvas,
		priority,
		performanceProfile,
		renderOptions,
		cleanup
	});
	applyProfileToScene(scenes.get(id)!, performanceProfile);
	resetPerformanceObservation();
	startRenderLoop();
}

export function unregisterScene(id: string, scene?: Scene): boolean {
	const renderable = scenes.get(id);
	if (!renderable) {
		return false;
	}
	if (scene && renderable.scene !== scene) {
		return false;
	}

	// Run cleanup registered for this scene
	renderable.cleanup();
	scenes.delete(id);

	if (scenes.size === 0) {
		stopRenderLoop();
	}
	return true;
}

export function disposeSceneRegistration(
	id: string,
	scene?: Scene,
	options: { disposeScene?: boolean } = {}
): boolean {
	const renderable = scenes.get(id);
	if (!renderable) {
		return false;
	}
	if (scene && renderable.scene !== scene) {
		return false;
	}

	scenes.delete(id);
	if (scenes.size === 0) {
		stopRenderLoop();
	}
	disposeRegisteredScene(renderable, options);
	return true;
}

export function getStats(): { sceneCount: number; targetFPS: number } {
	return {
		sceneCount: scenes.size,
		targetFPS: getCurrentTargetFPS()
	};
}

export function dispose(): void {
	stopRenderLoop();
	for (const id of [...scenes.keys()]) {
		disposeSceneRegistration(id);
	}
}
