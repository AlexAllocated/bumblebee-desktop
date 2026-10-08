import { createActor } from "xstate";
import type { Scene } from "@babylonjs/core/scene";
import { Constants } from "@babylonjs/core/Engines/constants";
import { Effect } from "@babylonjs/core/Materials/effect";
import { PostProcess } from "@babylonjs/core/PostProcesses/postProcess";
import "@babylonjs/core/Shaders/postprocess.vertex";
import { createScene, mountOverlayCanvas } from "../bumblebee/createScene";
import { surfaceMachine } from "./machines";
import type { OverlayOptions, OverlaySurfaceEffect } from "./types";
import { disposeSceneRegistration } from "../utils/renderManager";

const PHOSPHOR_SHADER_NAME = "bumblebeePhosphor";

Effect.RegisterShader(
	PHOSPHOR_SHADER_NAME,
	`varying vec2 vUV;
uniform sampler2D textureSampler;
uniform vec3 phosphorColor;

void main(void) {
	vec4 source = texture2D(textureSampler, vUV);
	float luminance = dot(source.rgb, vec3(0.299, 0.587, 0.114));
	vec3 phosphor = luminance * phosphorColor;
	gl_FragColor = vec4(phosphor, source.a);
}`
);

const PHOSPHOR_COLORS = {
	phosphor: [0.33, 1, 0.48],
	"phosphor-red": [0.65, 0.08, 0.06]
} as const satisfies Record<Exclude<OverlaySurfaceEffect, null>, readonly [number, number, number]>;

const createSurfaceId = () => {
	const randomId =
		globalThis.crypto && "randomUUID" in globalThis.crypto
			? globalThis.crypto.randomUUID()
			: Math.random().toString(36).slice(2);
	return `bumblebee-overlay:${randomId}`;
};

export class Surface {
	readonly actorRef = createActor(surfaceMachine).start();
	readonly scene: Scene;
	readonly #sceneId: string;
	readonly #zIndex: number;
	#effect: OverlaySurfaceEffect = null;
	#phosphorPostProcess: PostProcess | null = null;
	#disposed = false;
	#paused = false;

	constructor(options: Pick<OverlayOptions, "surface" | "zIndex">) {
		this.actorRef.send({ type: "CREATE" });
		this.#sceneId = createSurfaceId();
		this.#zIndex = options.zIndex ?? 2147482990;
		try {
			this.scene = createScene({
				container: options.surface?.container,
				id: this.#sceneId,
				zIndex: this.#zIndex,
				canvasResizeOptions: options.surface,
				shouldRender: () => !this.#paused
			});
		} catch (error) {
			this.actorRef.stop();
			throw error;
		}

		this.actorRef.send({ type: "READY" });
	}

	setPaused(paused: boolean) {
		this.#paused = paused;
		this.scene.animationsEnabled = !paused;
	}

	setContainer(container?: HTMLElement | null) {
		if (this.#disposed) return null;
		const engine = this.scene.getEngine();
		const canvas = engine.getRenderingCanvas();
		if (!canvas) return null;
		mountOverlayCanvas(canvas, container, this.#zIndex);
		engine.resize();
		if (!this.#paused) this.scene.render();
		return canvas;
	}

	setEffect(effect: OverlaySurfaceEffect) {
		if (this.#disposed || this.#effect === effect) return;
		const camera = this.scene.activeCamera;
		if (!camera) return;
		const previousEffect = this.#effect;
		this.#effect = effect;

		if (effect) {
			if (this.#phosphorPostProcess) {
				if (!previousEffect) camera.attachPostProcess(this.#phosphorPostProcess);
			} else {
				this.#phosphorPostProcess = new PostProcess(
					`${this.#sceneId}:phosphor`,
					PHOSPHOR_SHADER_NAME,
					["phosphorColor"],
					null,
					1,
					camera,
					Constants.TEXTURE_BILINEAR_SAMPLINGMODE,
					this.scene.getEngine(),
					false
				);
				this.#phosphorPostProcess.onApply = (shaderEffect) => {
					const color = PHOSPHOR_COLORS[this.#effect ?? "phosphor"];
					shaderEffect.setFloat3("phosphorColor", color[0], color[1], color[2]);
				};
			}
		} else if (this.#phosphorPostProcess) {
			camera.detachPostProcess(this.#phosphorPostProcess);
		}
	}

	dispose() {
		if (this.#disposed) return;
		this.#disposed = true;
		this.actorRef.send({ type: "DISPOSE" });
		this.#phosphorPostProcess?.dispose(this.scene.activeCamera ?? undefined);
		this.#phosphorPostProcess = null;
		if (!disposeSceneRegistration(this.#sceneId, this.scene) && !this.scene.isDisposed) {
			try {
				this.scene.dispose();
			} catch (error) {
				console.warn("Failed to dispose overlay surface scene:", error);
			}
		}
		this.actorRef.stop();
	}
}
