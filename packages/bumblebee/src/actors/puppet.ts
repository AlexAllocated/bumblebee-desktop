import { puppetTalkIntensityForFrame, speechOcclusionForFrame } from "../puppet/speechMotion";
import { createActor } from "xstate";
import { Scene } from "@babylonjs/core/scene";
import { Vector2, Vector3 } from "@babylonjs/core/Maths/math.vector";
import type { Observer } from "@babylonjs/core/Misc/observable";
import { puppetMachine } from "../overlay/machines";
import type {
	EventUnsubscribe,
	OverlayEvent,
	OverlayEventFor,
	OverlayEventSubscriptionOptions,
	OverlayEventType,
	PositionTarget,
	Puppet,
	PuppetImageMask,
	PuppetOptions,
	PuppetNameplate,
	PuppetPose,
	PuppetShowOptions,
	PuppetTransitionPhase,
	PuppetTransitionSoundHandler,
	Speech
} from "../overlay/types";
import type { OverlayRuntime } from "../overlay/runtime";
import type { BubbleScreenRect } from "../overlay/chatBubbles";
import { createPuppet as createPuppetModel, type PuppetController } from "../puppet";
import type { SpeechMotionFrame } from "../utils/speechMotion";

const isHTMLElement = (value: unknown): value is HTMLElement =>
	typeof HTMLElement !== "undefined" && value instanceof HTMLElement;

const positionToPuppetSpawn = (target: PositionTarget) => {
	if (isHTMLElement(target)) {
		const rect = target.getBoundingClientRect();
		return { horizontalPercent: ((rect.left + rect.width / 2) / window.innerWidth) * 100 };
	}
	if (target instanceof Vector2) {
		return { horizontalPercent: (target.x / window.innerWidth) * 100 };
	}
	if (target instanceof Vector3) {
		return { horizontalPercent: Math.max(0, Math.min(100, target.x * 100)) };
	}
	return { horizontalPercent: target.x * 100 };
};

export const puppetPoseFromOptions = (options?: PuppetOptions): PuppetPose => ({
	...(options?.position ? { position: positionToPuppetSpawn(options.position) } : {}),
	...(typeof options?.scale === "number" ? { scale: options.scale } : {}),
	...(typeof options?.occlusion === "number" ? { occlusion: options.occlusion } : {})
});

const PUPPET_TRANSITION_SOUND_VOLUME = 0.12;
const PUPPET_TRANSITION_SOUND_PLAYBACK_RATE = 0.72;

export class PuppetActor implements Puppet {
	readonly actorRef = createActor(puppetMachine).start();
	readonly actorId: string;
	readonly cacheKey: string;
	controller!: PuppetController;
	#scale = 0.5;
	#occlusion = 0.25;
	#stickColor?: string;
	#imageUrl?: string;
	#imageMask?: PuppetImageMask;
	#position: PositionTarget = { x: 0.76, y: 0.82 };
	#appearance: "normal" | "ghost" = "normal";
	#nameplate: PuppetNameplate | null = null;
	#nameplateObserver: Observer<Scene> | null = null;
	#autoHideAfterSpeech = false;
	#speechOcclusionMotion = true;
	#transitionSoundHandler: PuppetTransitionSoundHandler | null = null;
	#speechOcclusionBaseline: ReturnType<PuppetController["captureSpeechOcclusionBaseline"]> | null =
		null;
	#disposed = false;

	constructor(
		readonly runtime: OverlayRuntime,
		readonly id: string,
		options?: PuppetOptions
	) {
		const instanceId = options?.instanceId?.trim();
		this.cacheKey = instanceId || id;
		this.actorId = `puppet:${this.cacheKey}`;
		this.#stickColor = options?.stickColor;
		this.#imageUrl = options?.imageUrl;
		this.#imageMask = options?.imageMask;
		this.#scale = options?.scale ?? this.#scale;
		this.#occlusion = options?.occlusion ?? this.#occlusion;
		this.#position = options?.position ?? this.#position;
		this.#appearance = options?.appearance ?? this.#appearance;
		this.#speechOcclusionMotion = options?.speechOcclusionMotion ?? true;
		this.#nameplate = this.#normalizeNameplateOption(options?.nameplate);
	}

	get speechTarget() {
		return {
			type: "puppet" as const,
			id: this.id,
			actorId: this.actorId,
			instanceId: this.cacheKey === this.id ? undefined : this.cacheKey,
			imageUrl: this.#imageUrl,
			imageMask: this.#imageMask
		};
	}

	get stickColor() {
		return this.#stickColor ?? null;
	}

	async init(options?: PuppetOptions) {
		const transitionSoundReady = this.runtime
			.preloadSounds(["transitionWhoosh"])
			.catch((error) => console.warn("Chat puppet transition sound unavailable", error));
		if (options?.stickColor) this.#stickColor = options.stickColor;
		if ("imageUrl" in (options ?? {})) this.#imageUrl = options?.imageUrl;
		if ("imageMask" in (options ?? {})) this.#imageMask = options?.imageMask;
		if (typeof options?.scale === "number") this.#scale = options.scale;
		if (typeof options?.occlusion === "number") this.#occlusion = options.occlusion;
		if (options?.position) this.#position = options.position;
		if (options?.appearance) this.#appearance = options.appearance;
		if (typeof options?.speechOcclusionMotion === "boolean") {
			this.#speechOcclusionMotion = options.speechOcclusionMotion;
		}
		if ("nameplate" in (options ?? {})) {
			this.#nameplate = this.#normalizeNameplateOption(options?.nameplate);
		}
		this.controller = await createPuppetModel({
			puppetId: this.id,
			imageUrl: this.#imageUrl ?? this.runtime.assets.puppetImage(this.id),
			imageMask: this.#imageMask,
			scene: this.runtime.puppetSurface.scene,
			puppetPosition: positionToPuppetSpawn(this.#position),
			scalePercentage: this.#scale,
			occlusionPercentage: this.#occlusion,
			stickColor: this.#stickColor,
			appearance: this.#appearance
		});
		this.#startNameplateRefresh();
		this.actorRef.send({ type: "READY" });
		await transitionSoundReady;
		if (options?.visible) await this.show();
		return this;
	}

	usesVisualSource(options?: PuppetOptions) {
		const imageUrl = options && "imageUrl" in options ? options.imageUrl : undefined;
		const imageMask = options && "imageMask" in options ? options.imageMask : undefined;
		const requestedImageUrl = imageUrl ?? null;
		const currentImageUrl = this.#imageUrl ?? null;
		const requestedImageMask = imageMask ?? "none";
		const currentImageMask = this.#imageMask ?? "none";
		const hasStickColor = options && "stickColor" in options;
		const requestedStickColor = hasStickColor ? (options.stickColor ?? null) : null;
		const currentStickColor = this.#stickColor ?? null;
		return (
			requestedImageUrl === currentImageUrl &&
			requestedImageMask === currentImageMask &&
			(!hasStickColor || requestedStickColor === currentStickColor)
		);
	}

	async configure(options?: PuppetOptions) {
		if (!options) return;
		const pose = puppetPoseFromOptions(options);
		if (pose.position || typeof pose.scale === "number" || typeof pose.occlusion === "number") {
			await this.setPose(pose, { animate: false, durationMs: 1 });
		}
		if (options.appearance) this.setGhostMode(options.appearance === "ghost");
		if (typeof options.speechOcclusionMotion === "boolean") {
			this.#speechOcclusionMotion = options.speechOcclusionMotion;
		}
		if ("nameplate" in options) {
			this.setNameplate(this.#normalizeNameplateOption(options.nameplate));
		}
	}

	async show(options?: PuppetShowOptions) {
		this.actorRef.send({ type: "SHOW" });
		void this.#playTransitionSound("show");
		await this.controller.show(options);
		this.#renderNameplate();
		this.actorRef.send({ type: "SHOWN" });
	}

	async hide() {
		this.actorRef.send({ type: "HIDE" });
		void this.#playTransitionSound("hide");
		await this.controller.hide();
		this.runtime.nameplates.hide(this.actorId);
		this.actorRef.send({ type: "HIDDEN" });
	}

	async #playTransitionSound(phase: PuppetTransitionPhase) {
		try {
			if (this.#transitionSoundHandler) {
				await this.#transitionSoundHandler(phase);
				return;
			}
			await this.runtime.playSound("transitionWhoosh", PUPPET_TRANSITION_SOUND_VOLUME, {
				playbackRate: PUPPET_TRANSITION_SOUND_PLAYBACK_RATE,
				reverse: phase === "show"
			});
		} catch {
			// Puppet animation should remain uninterrupted when audio is unavailable.
		}
	}

	setTransitionSoundHandler(handler: PuppetTransitionSoundHandler | null) {
		this.#transitionSoundHandler = handler;
	}

	async moveTo(target: PositionTarget) {
		this.#position = target;
		this.actorRef.send({ type: "MOVE" });
		this.controller.setPuppetPosition(positionToPuppetSpawn(target), {
			animate: true,
			durationMs: 700
		});
		await this.controller.moveToSpawn({
			targetScale: this.#scale,
			durationMs: 700
		});
		this.actorRef.send({ type: "MOVED" });
	}

	async say(textOrSpeech: string | Omit<Speech, "target">) {
		await this.runtime.say({
			...(typeof textOrSpeech === "string" ? { text: textOrSpeech } : textOrSpeech),
			target: this
		});
	}

	setStickColor(color: string) {
		this.#stickColor = color;
	}

	setOpacity(value: number) {
		this.controller.setOpacity(value);
	}

	setDimmed(enabled: boolean) {
		this.controller.setDimmed(enabled);
	}

	setRenderGroupOffset(offset: number) {
		this.controller.setRenderGroupOffset?.(offset);
	}

	isEnabled() {
		return this.controller.node.isEnabled();
	}

	applyOrganicTilt(options?: { disableTilt?: boolean; animate?: boolean; durationMs?: number }) {
		return this.controller.applyOrganicTilt(options);
	}

	cancelOrganicTiltTransition() {
		this.controller.cancelOrganicTiltTransition?.();
	}

	setScale(scale: number) {
		this.#scale = scale;
		this.controller.setScalePercentage(scale);
	}

	async setPose(pose: PuppetPose, options?: { animate?: boolean; durationMs?: number }) {
		const animate = !!options?.animate;
		const durationMs = options?.durationMs ?? 700;
		let needsMove = false;
		if (pose.position) {
			this.#position = { x: pose.position.horizontalPercent / 100, y: 0.82 };
			this.controller.setPuppetPosition(pose.position, { animate, durationMs });
			needsMove = true;
		}
		if (typeof pose.scale === "number") {
			this.#scale = pose.scale;
			if (!animate) this.controller.setScalePercentage(pose.scale);
			needsMove = true;
		}
		if (typeof pose.occlusion === "number") {
			this.#occlusion = pose.occlusion;
			await this.controller.setOcclusionRatio(pose.occlusion, {
				animate,
				durationMs,
				deferMove: animate
			});
			needsMove = true;
		}
		if (!needsMove) return;
		await this.controller.moveToSpawn({
			targetScale: typeof pose.scale === "number" ? pose.scale : this.#scale,
			durationMs: animate ? durationMs : 1
		});
	}

	setGhostMode(enabled: boolean) {
		this.#appearance = enabled ? "ghost" : "normal";
		this.controller.setGhostMode?.(enabled);
	}

	setNameplate(nameplate: PuppetNameplate | null) {
		this.#nameplate = nameplate;
		this.#renderNameplate();
	}


	getLocalSize() {
		const dims = this.controller.node.metadata?.localDims as
			{ w?: number; h?: number } | null | undefined;
		if (!dims?.w || !dims?.h) return null;
		return { width: dims.w, height: dims.h };
	}

	setFacing(
		facing: "auto" | "left" | "right",
		options?: { animate?: boolean; durationMs?: number }
	) {
		if (facing === "auto") {
			void this.controller.alignFacingWithPosition({
				animate: options?.animate ?? true,
				durationMs: options?.durationMs
			});
			this.actorRef.send({ type: "FACE_AUTO" });
			return;
		}
		this.controller.setFacingLeft(facing === "left");
		this.actorRef.send({ type: facing === "left" ? "FACE_LEFT" : "FACE_RIGHT" });
	}

	startTalking() {
		this.actorRef.send({ type: "TALK" });
		if (this.#speechOcclusionMotion) this.#captureSpeechBaseline();
		void this.controller.talk();
	}

	stopTalking() {
		this.actorRef.send({ type: "SHUTUP" });
		void this.settleSpeechMotion();
		void this.controller.shutup();
	}

	updateSpeechMotion(frame: SpeechMotionFrame) {
		this.controller.setTalkingSpeed?.(frame.talkSpeed);
		this.controller.setTalkingIntensity?.(puppetTalkIntensityForFrame(frame));
		if (!this.#speechOcclusionMotion) return;
		const baseline = this.#captureSpeechBaseline();
		this.controller.applySpeechOcclusionRatio(
			speechOcclusionForFrame(baseline.occlusionRatio, frame.normalizedLevel),
			baseline
		);
	}

	async settleSpeechMotion() {
		const baseline = this.#speechOcclusionBaseline;
		if (baseline) {
			this.controller.applySpeechOcclusionRatio(baseline.occlusionRatio, baseline);
			this.#speechOcclusionBaseline = null;
		}
		this.controller.setTalkingSpeed?.(1);
		this.controller.setTalkingIntensity?.(0);
	}

	setAutoHideAfterSpeech(enabled: boolean) {
		this.#autoHideAfterSpeech = enabled;
	}

	shouldHideAfterSpeech() {
		return this.#autoHideAfterSpeech;
	}

	getChatBubbleScreenRect(): BubbleScreenRect | null {
		return this.controller.getChatDecorationScreenRect({ stable: true }) ?? null;
	}

	on<T extends OverlayEventType>(
		event: T,
		handler: (event: OverlayEventFor<T>) => void,
		options?: OverlayEventSubscriptionOptions
	): EventUnsubscribe {
		return this.runtime.events.on(event, handler, options);
	}

	async dispose() {
		if (this.#disposed) return;
		this.#disposed = true;
		this.actorRef.send({ type: "DISPOSE" });
		this.#stopNameplateRefresh();
		this.runtime.nameplates.remove(this.actorId);
		await this.controller.dispose();
		this.actorRef.send({ type: "DISPOSED" });
		this.actorRef.stop();
		this.runtime.events.emit({
			type: "actor:disposed",
			actorId: this.actorId,
			actorType: "puppet"
		} satisfies OverlayEvent);
		this.runtime.forgetPuppet(this);
	}

	#captureSpeechBaseline() {
		if (!this.#speechOcclusionBaseline) {
			this.#speechOcclusionBaseline = this.controller.captureSpeechOcclusionBaseline();
		}
		return this.#speechOcclusionBaseline;
	}

	#normalizeNameplateOption(value: PuppetOptions["nameplate"]): PuppetNameplate | null {
		if (!value) return null;
		if (value === true) return { text: "" };
		return value;
	}

	#renderNameplate() {
		if (!this.#nameplate?.text?.trim()) {
			this.runtime.nameplates.hide(this.actorId);
			return;
		}
		const anchor = this.#getNameplateAnchor();
		this.runtime.nameplates.show(this.actorId, this.#nameplate, anchor);
	}

	#refreshNameplateGeometry() {
		if (!this.#nameplate?.text?.trim()) return;
		const anchor = this.#getNameplateAnchor();
		this.runtime.nameplates.updateGeometry(this.actorId, anchor);
	}

	#getNameplateAnchor() {
		const anchor = this.controller.getNameplateScreenAnchor?.({ stable: false }) ?? null;
		if (!anchor) return null;
		return {
			...anchor,
			constrainToViewport: !this.actorRef.getSnapshot().matches("hiding")
		};
	}

	#startNameplateRefresh() {
		this.#stopNameplateRefresh();
		this.#nameplateObserver = this.controller.node
			.getScene()
			.onAfterRenderObservable.add(() => this.#refreshNameplateGeometry());
	}

	#stopNameplateRefresh() {
		if (!this.#nameplateObserver) return;
		this.controller.node.getScene().onAfterRenderObservable.remove(this.#nameplateObserver);
		this.#nameplateObserver = null;
	}
}
