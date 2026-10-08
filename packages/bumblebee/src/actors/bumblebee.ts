import { createActor } from "xstate";
import type { AnimationGroup } from "@babylonjs/core/Animations/animationGroup";
import type { Observer } from "@babylonjs/core/Misc/observable";
import type { Scene } from "@babylonjs/core/scene";
import type { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import { Matrix, Quaternion, Vector2, Vector3 } from "@babylonjs/core/Maths/math.vector";
import { Viewport } from "@babylonjs/core/Maths/math.viewport";
import { bumblebeeMachine } from "../overlay/machines";
import type {
	Bumblebee,
	BumblebeeAttachmentPoint,
	BumblebeeAnchorMode,
	BumblebeeEmote,
	BumblebeeOptions,
	BumblebeeScaleReference,
	EventUnsubscribe,
	MoveAlongOptions,
	MoveTarget,
	OverlayEvent,
	OverlayEventFor,
	OverlayEventSubscriptionOptions,
	OverlayEventType,
	PlaceTarget,
	PositionTarget,
	Speech
} from "../overlay/types";
import type { OverlayRuntime } from "../overlay/runtime";
import { loadModel } from "../bumblebee/loadModel";
import { playAnimation } from "../utils/playAnimation";
import { createMoveAlongAnimation, createMoveToAnimation } from "../bumblebee/animations/moveTo";
import {
	captureNodeLocalBounds,
	projectNodeLocalBoundsToScreenRect,
	projectNodeToScreenRect,
	type NodeLocalBounds
} from "../utils/screenRect";
import { resolveCanvasViewportSize, screenToWorld } from "../utils/screenToWorld";
import type { BubbleScreenRect } from "../overlay/chatBubbles";
import type { SpeechMotionFrame } from "../utils/speechMotion";
import { BUMBLEBEE_TALK_WEIGHT } from "../bumblebee/speechMotion";
import { resolveBumblebeeEmoteLoopRange } from "../bumblebee/emote";
import { createMoveScrollFollower, type MoveScrollFollower } from "../utils/moveScrollFollower";

const animationNamed = (groups: AnimationGroup[], name: string) => {
	const group = groups.find((candidate) => candidate.name === name);
	if (!group) throw new Error(`Missing Bumblebee animation: ${name}`);
	return group;
};

const isHTMLElement = (value: unknown): value is HTMLElement =>
	typeof HTMLElement !== "undefined" && value instanceof HTMLElement;

const isMoveOptions = (
	target: MoveTarget
): target is Extract<MoveTarget, { destination: unknown }> =>
	typeof target === "object" &&
	target !== null &&
	"destination" in target &&
	!(target instanceof Vector2) &&
	!(target instanceof Vector3) &&
	!isHTMLElement(target);

const isPlaceOptions = (
	target: PlaceTarget
): target is Extract<PlaceTarget, { destination: PositionTarget }> =>
	typeof target === "object" &&
	target !== null &&
	"destination" in target &&
	!(target instanceof Vector2) &&
	!(target instanceof Vector3) &&
	!isHTMLElement(target);

const isFiniteVector3 = (vector: Vector3) =>
	Number.isFinite(vector.x) && Number.isFinite(vector.y) && Number.isFinite(vector.z);

const SAME_MOVE_POSITION_EPSILON = 0.0001;
const SAME_MOVE_VALUE_EPSILON = 0.000001;

const sameOptionalMoveValue = (current: number | undefined, next: number | undefined) =>
	current === next ||
	(typeof current === "number" &&
		typeof next === "number" &&
		Math.abs(current - next) <= SAME_MOVE_VALUE_EPSILON);

const quaternionDot = (a: Quaternion, b: Quaternion) =>
	a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;

const alignQuaternionSign = (quaternion: Quaternion, reference: Quaternion) =>
	quaternionDot(quaternion, reference) < 0
		? new Quaternion(-quaternion.x, -quaternion.y, -quaternion.z, -quaternion.w)
		: quaternion;

const nodeQuaternion = (node: TransformNode) =>
	node.rotationQuaternion?.clone() ??
	Quaternion.FromEulerAngles(node.rotation.x, node.rotation.y, node.rotation.z);

export const shouldApplyAnchoredLerp = ({
	anchorModeEnabled,
	hasAnchor,
	responsiveMoveActive
}: {
	anchorModeEnabled: boolean;
	hasAnchor: boolean;
	responsiveMoveActive: boolean;
}) => anchorModeEnabled && hasAnchor && !responsiveMoveActive;

const ANCHOR_LERP_REFERENCE_FRAME_MS = 1000 / 60;

export const resolveFrameRateIndependentLerpFactor = (
	perFrameFactor: number,
	elapsedMs: number
) => {
	const normalizedFactor = Math.max(0, Math.min(1, perFrameFactor));
	if (!Number.isFinite(elapsedMs) || elapsedMs <= 0) return normalizedFactor;
	return 1 - Math.pow(1 - normalizedFactor, elapsedMs / ANCHOR_LERP_REFERENCE_FRAME_MS);
};

export class BumblebeeActor implements Bumblebee {
	readonly id = "bumblebee" as const;
	readonly actorId = "bumblebee:default";
	readonly speechTarget = { type: "bumblebee" as const };
	readonly actorRef = createActor(bumblebeeMachine).start();
	node!: TransformNode;
	animationGroups: AnimationGroup[] = [];
	#stanceAbort: AbortController | null = null;
	#talkAbort: AbortController | null = null;
	#lastAnchorElement: HTMLElement | null = null;
	#lastMoveScale = 1;
	#lastMoveBounds: { width?: number; height?: number } = {};
	#lastScaleReference: BumblebeeScaleReference | null = null;
	#defaultScaleReference: BumblebeeScaleReference | null = null;
	#frontFootNodes: TransformNode[] = [];
	#stableChatBubbleBounds: NodeLocalBounds | null = null;
	#anchorModeEnabled = false;
	// This tracks the temporary flying stance around scroll activity. It must never gate
	// position tracking: browsers may coalesce scroll events while a scroll is in progress.
	#anchorActive = false;
	#anchorNeedsLanding = false;
	#anchorSettleMs = 400;
	#anchorThrottleMs = 100;
	#anchorLerpFactor = 0.35;
	#anchorTimeout: ReturnType<OverlayRuntime["setPlaybackTimeout"]> | null = null;
	#anchorThrottleTimeout: ReturnType<OverlayRuntime["setPlaybackTimeout"]> | null = null;
	#anchorResizeFrame: number | null = null;
	#anchorLastRequest = 0;
	#anchorLastFrameTime = 0;
	#responsiveAnchorMoveActive = false;
	#responsiveAnchorMoveAbort: AbortController | null = null;
	#beforeRenderObserver: Observer<Scene> | null = null;
	#disposed = false;
	#requestAnchorLerp = () => this.#requestAnchoredLerp();
	#requestAnchorLerpAfterLayoutChange = () => {
		if (this.#anchorResizeFrame !== null) return;
		this.#anchorResizeFrame = requestAnimationFrame(() => {
			this.#anchorResizeFrame = null;
			this.#requestAnchoredLerp();
		});
	};

	constructor(readonly runtime: OverlayRuntime) {}

	async init(options?: BumblebeeOptions) {
		this.configure(options);
		const loaded = await loadModel({
			scene: this.runtime.surface.scene,
			modelUrl: this.runtime.assets.bumblebeeModel()
		});
		this.node = loaded.node;
		this.animationGroups = loaded.animationGroups;
		this.#frontFootNodes = loaded.frontFootNodes;
		this.#stableChatBubbleBounds = captureNodeLocalBounds(this.node);
		this.actorRef.send({ type: "READY" });
		if (options?.visible ?? true) {
			await this.show();
		} else {
			this.node.setEnabled(false);
		}
		return this;
	}

	configure(options?: BumblebeeOptions) {
		if (!options) return;
		if ("scaleReference" in options) {
			this.#defaultScaleReference = options.scaleReference ?? null;
			this.#lastScaleReference = options.scaleReference ?? this.#lastScaleReference;
		}
		if ("anchorMode" in options && options.anchorMode !== undefined) {
			this.setAnchorMode(options.anchorMode);
		}
	}

	async show() {
		const wasVisible = this.#isVisible();
		this.node.setEnabled(true);
		this.actorRef.send({ type: "SHOW" });
		if (!wasVisible) this.#playLoop("standing");
	}

	async hide() {
		this.node.setEnabled(false);
		this.actorRef.send({ type: "HIDE" });
	}

	async fly() {
		this.actorRef.send({ type: "FLY" });
		this.#playLoop("idleFlying", { speed: 1.25 });
	}

	async land() {
		this.actorRef.send({ type: "LAND" });
		this.#playLoop("standing");
	}



	async placeAt(target: PlaceTarget) {
		const options = isPlaceOptions(target) ? target : { destination: target };
		const scalePercentage =
			typeof options.scalePercentage === "number" ? options.scalePercentage : this.#lastMoveScale;
		const destination = options.destination;
		const scaleReference =
			options.scaleReference ?? this.#defaultScaleReference ?? this.#lastScaleReference;
		const { width, height } = this.#sampleReference(scaleReference);
		const normalizedWidth = this.#normalizeDimension(width ?? this.#lastMoveBounds.width);
		const normalizedHeight = this.#normalizeDimension(height ?? this.#lastMoveBounds.height);

		this.#lastAnchorElement = isHTMLElement(destination) ? destination : null;
		this.#lastMoveScale = scalePercentage;
		this.#lastScaleReference = scaleReference ?? null;
		this.#lastMoveBounds = { width: normalizedWidth, height: normalizedHeight };

		this.node.position = this.#worldDestination(
			destination,
			scalePercentage,
			normalizedWidth,
			normalizedHeight
		);
		this.#faceCamera();

		if (options.visible === false) {
			await this.hide();
		} else if (options.visible === true || (options.stance && !this.#isVisible())) {
			await this.show();
		}
		if (options.visible !== false && options.stance === "flying") {
			await this.fly();
		} else if (options.visible !== false && options.stance === "standing") {
			await this.land();
		}
		this.#refreshActiveBubbleGeometry();
	}

	async moveTo(target: MoveTarget) {
		const options = isMoveOptions(target) ? target : { destination: target };
		const previousScalePercentage = this.#lastMoveScale;
		const previousBounds = this.#lastMoveBounds;
		const scalePercentage =
			typeof options.scalePercentage === "number" ? options.scalePercentage : this.#lastMoveScale;
		const destination = options.destination;
		const scaleReference =
			options.scaleReference ?? this.#defaultScaleReference ?? this.#lastScaleReference;
		const { width, height } = this.#sampleReference(scaleReference);
		const normalizedWidth = this.#normalizeDimension(width ?? this.#lastMoveBounds.width);
		const normalizedHeight = this.#normalizeDimension(height ?? this.#lastMoveBounds.height);
		const elementDestination = isHTMLElement(destination) ? destination : null;
		const scrollFollower = elementDestination
			? createMoveScrollFollower(elementDestination, options.scrollIntoView)
			: null;
		const worldDestination = this.#worldDestination(
			destination,
			scalePercentage,
			normalizedWidth,
			normalizedHeight
		);
		const alreadyAtDestination =
			isFiniteVector3(this.node.position) &&
			isFiniteVector3(worldDestination) &&
			Vector3.DistanceSquared(this.node.position, worldDestination) <=
				SAME_MOVE_POSITION_EPSILON * SAME_MOVE_POSITION_EPSILON &&
			Math.abs(previousScalePercentage - scalePercentage) <= SAME_MOVE_VALUE_EPSILON &&
			sameOptionalMoveValue(previousBounds.width, normalizedWidth) &&
			sameOptionalMoveValue(previousBounds.height, normalizedHeight);

		this.#lastAnchorElement = elementDestination;
		this.#lastMoveScale = scalePercentage;
		this.#lastScaleReference = scaleReference ?? null;
		this.#lastMoveBounds = { width: normalizedWidth, height: normalizedHeight };
		if (alreadyAtDestination && !scrollFollower?.willScroll) {
			this.#faceCamera();
			this.#refreshActiveBubbleGeometry();
			return;
		}
		const startedFlying = this.#isFlying();
		const curveIntensity =
			typeof options.curveIntensity === "number"
				? this.#normalizeRange(options.curveIntensity, 0, 1, this.#moveCurveIntensity())
				: this.#moveCurveIntensity();
		this.actorRef.send({ type: "MOVE" });
		this.actorRef.send({ type: "MOVE_START" });
		this.#playMoveLoop(startedFlying);
		const bubbleRefreshObserver = this.#trackBubbleGeometryDuringMove();
		try {
			if ((this.#anchorModeEnabled || scrollFollower) && elementDestination) {
				await this.#playResponsiveAnchorMove({
					destination: elementDestination,
					scalePercentage,
					boundsWidth: normalizedWidth,
					boundsHeight: normalizedHeight,
					speed: options.speed,
					scrollFollower
				});
			} else {
				const group = createMoveToAnimation({
					node: this.node,
					destination: this.#animationDestination(destination),
					scalePercentage,
					boundsWidth: normalizedWidth,
					boundsHeight: normalizedHeight,
					curveIntensity
				});
				try {
					await playAnimation({
						animationGroup: group,
						scene: this.runtime.surface.scene,
						speed: options.speed,
						strictEnd: true
					});
				} finally {
					group.dispose();
				}
			}
		} finally {
			this.#removeBubbleRefreshObserver(bubbleRefreshObserver);
			this.#refreshActiveBubbleGeometry();
		}
		if (this.#disposed) return;
		this.actorRef.send({ type: "MOVE_END" });
		this.actorRef.send({ type: "STOP" });
		this.#playCurrentStanceLoop();
		this.#scheduleAnchoredLerp();
	}

	async moveAlong(targets: PositionTarget[], options?: MoveAlongOptions) {
		if (!targets.length) return;
		const startScalePercentage =
			typeof options?.startScalePercentage === "number"
				? options.startScalePercentage
				: this.#lastMoveScale;
		const scalePercentage =
			typeof options?.scalePercentage === "number" ? options.scalePercentage : this.#lastMoveScale;
		const scaleReference =
			options?.scaleReference ?? this.#defaultScaleReference ?? this.#lastScaleReference;
		const { width, height } = this.#sampleReference(scaleReference);
		const normalizedWidth = this.#normalizeDimension(width ?? this.#lastMoveBounds.width);
		const normalizedHeight = this.#normalizeDimension(height ?? this.#lastMoveBounds.height);
		const finalDestination = targets.at(-1);
		const elementDestination =
			finalDestination && isHTMLElement(finalDestination) ? finalDestination : null;

		this.#lastAnchorElement = elementDestination;
		this.#lastMoveScale = scalePercentage;
		this.#lastScaleReference = scaleReference ?? null;
		this.#lastMoveBounds = { width: normalizedWidth, height: normalizedHeight };
		const startedFlying = this.#isFlying();
		this.actorRef.send({ type: "MOVE" });
		this.actorRef.send({ type: "MOVE_START" });
		this.#playMoveLoop(startedFlying);
		const group = createMoveAlongAnimation({
			node: this.node,
			destinations: targets.map((destination) => this.#animationDestination(destination)),
			startScalePercentage,
			scalePercentage,
			boundsWidth: normalizedWidth,
			boundsHeight: normalizedHeight
		});
		const bubbleRefreshObserver = this.#trackBubbleGeometryDuringMove();
		try {
			await playAnimation({
				animationGroup: group,
				scene: this.runtime.surface.scene,
				speed: options?.speed,
				strictEnd: true
			});
		} finally {
			this.#removeBubbleRefreshObserver(bubbleRefreshObserver);
			this.#refreshActiveBubbleGeometry();
			group.dispose();
		}
		if (this.#disposed) return;
		this.actorRef.send({ type: "MOVE_END" });
		this.actorRef.send({ type: "STOP" });
		this.#playCurrentStanceLoop();
		this.#scheduleAnchoredLerp();
	}

	setAnchorMode(mode: BumblebeeAnchorMode) {
		if (typeof mode === "boolean") {
			if (mode) {
				this.#enableAnchorMode();
			} else {
				this.#disableAnchorMode();
			}
			return;
		}

		if ("scaleReference" in mode) {
			this.#defaultScaleReference = mode.scaleReference ?? null;
			this.#lastScaleReference = mode.scaleReference ?? this.#lastScaleReference;
		}
		this.#anchorSettleMs = this.#normalizePositive(mode.settleMs, this.#anchorSettleMs);
		this.#anchorThrottleMs = this.#normalizePositive(mode.throttleMs, this.#anchorThrottleMs);
		this.#anchorLerpFactor = this.#normalizeRange(mode.lerpFactor, 0.01, 1, this.#anchorLerpFactor);

		if (mode.enabled ?? true) {
			this.#enableAnchorMode();
		} else {
			this.#disableAnchorMode();
		}
	}

	async say(textOrSpeech: string | Omit<Speech, "target">) {
		await this.runtime.say({
			...(typeof textOrSpeech === "string" ? { text: textOrSpeech } : textOrSpeech),
			target: this
		});
	}

	async emote(
		name: BumblebeeEmote,
		duration?: number,
		loop?: boolean,
		signal?: AbortSignal,
		weight = 0.5
	) {
		const loopRange = loop ? resolveBumblebeeEmoteLoopRange(name) : undefined;
		await playAnimation({
			animationGroup: animationNamed(this.animationGroups, name),
			scene: this.runtime.surface.scene,
			blendMode: "additive",
			weight,
			duration,
			loop,
			signal,
			...loopRange,
			resetToInitialOnStop: true
		});
	}

	async backflip() {
		await playAnimation({
			animationGroup: animationNamed(this.animationGroups, "backflip"),
			scene: this.runtime.surface.scene,
			blendMode: "additive",
			weight: 1
		});
	}

	startTalking() {
		this.actorRef.send({ type: "TALK" });
		if (this.#talkAbort) return;
		const abort = new AbortController();
		this.#talkAbort = abort;
		const standing = this.actorRef.getSnapshot().matches({
			visible: { stance: "standing" }
		});
		void playAnimation({
			animationGroup: animationNamed(
				this.animationGroups,
				standing ? "standingTalking" : "talking"
			),
			scene: this.runtime.surface.scene,
			loop: true,
			blendMode: "additive",
			weight: BUMBLEBEE_TALK_WEIGHT,
			signal: abort.signal
		}).catch(() => undefined);
	}

	stopTalking() {
		this.actorRef.send({ type: "SHUTUP" });
		this.#talkAbort?.abort();
		this.#talkAbort = null;
	}

	updateSpeechMotion(frame: SpeechMotionFrame) {
		const standingTalk = this.animationGroups.find(
			(animation) => animation.name === "standingTalking"
		);
		const flyingTalk = this.animationGroups.find((animation) => animation.name === "talking");
		if (standingTalk) standingTalk.speedRatio = frame.talkSpeed;
		if (flyingTalk) flyingTalk.speedRatio = frame.talkSpeed;
	}

	getScreenRect(): BubbleScreenRect | null {
		return projectNodeToScreenRect(this.node);
	}

	getScalePercentage() {
		return this.#lastMoveScale;
	}

	getAttachmentPoint(name: BumblebeeAttachmentPoint): { x: number; y: number } | null {
		if (name !== "feet") return null;
		const scene = this.runtime.surface.scene;
		const camera = scene.activeCamera;
		const engine = scene.getEngine();
		const canvas = engine.getRenderingCanvas();
		if (!camera || !canvas || this.#frontFootNodes.length === 0) return null;
		const { cssWidth, cssHeight } = resolveCanvasViewportSize(canvas, engine);
		if (!cssWidth || !cssHeight) return null;
		const viewport = new Viewport(0, 0, cssWidth, cssHeight);
		const transform = scene.getTransformMatrix();
		const projected = this.#frontFootNodes
			.map((node) => {
				node.computeWorldMatrix(true);
				return Vector3.Project(node.getAbsolutePosition(), Matrix.Identity(), transform, viewport);
			})
			.filter((point) => Number.isFinite(point.x) && Number.isFinite(point.y));
		if (projected.length === 0) return null;
		const x = projected.reduce((sum, point) => sum + point.x, 0) / projected.length;
		const y = projected.reduce((sum, point) => sum + point.y, 0) / projected.length;
		const canvasRect = canvas.getBoundingClientRect();
		return { x: x + canvasRect.left, y: y + canvasRect.top };
	}

	getChatBubbleScreenRect(): BubbleScreenRect | null {
		const bodyRect = this.#stableChatBubbleBounds
			? projectNodeLocalBoundsToScreenRect(this.node, this.#stableChatBubbleBounds)
			: this.getScreenRect();
		if (!bodyRect) return null;
		const mouthX = bodyRect.centerX;
		const mouthY = bodyRect.top + bodyRect.height * 0.58;
		return {
			left: mouthX - bodyRect.width / 2,
			right: mouthX + bodyRect.width / 2,
			top: mouthY - bodyRect.height / 2,
			bottom: mouthY + bodyRect.height / 2,
			width: bodyRect.width,
			height: bodyRect.height,
			centerX: mouthX,
			centerY: mouthY
		};
	}

	isAnchoredTo(element: HTMLElement | null | undefined) {
		return Boolean(element && this.#lastAnchorElement === element);
	}

	getLastAnchor() {
		return this.#lastAnchorElement;
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
		this.#disableAnchorMode();
		this.actorRef.send({ type: "DISPOSE" });
		this.#responsiveAnchorMoveAbort?.abort();
		this.#responsiveAnchorMoveAbort = null;
		this.#stanceAbort?.abort();
		this.stopTalking();
		this.node.dispose();
		for (const group of this.animationGroups) group.dispose();
		this.actorRef.send({ type: "DISPOSED" });
		this.actorRef.stop();
		this.runtime.events.emit({
			type: "actor:disposed",
			actorId: this.actorId,
			actorType: "bumblebee"
		} satisfies OverlayEvent);
		this.runtime.forgetBumblebee(this);
	}

	#playLoop(
		name: string,
		options: {
			speed?: number;
			blendMode?: "none" | "blend" | "additive";
			weight?: number;
		} = {}
	) {
		this.#stanceAbort?.abort();
		const abort = new AbortController();
		this.#stanceAbort = abort;
		void playAnimation({
			animationGroup: animationNamed(this.animationGroups, name),
			scene: this.runtime.surface.scene,
			loop: true,
			speed: options.speed,
			blendMode: options.blendMode,
			weight: options.weight,
			signal: abort.signal
		}).catch(() => undefined);
	}

	#playMoveLoop(startedFlying: boolean) {
		if (startedFlying) {
			this.#playLoop("activeFlying", { speed: 1.25 });
			return;
		}
		this.#playLoop("walking", { blendMode: "additive", weight: 1, speed: 1.25 });
	}

	#playCurrentStanceLoop() {
		if (this.#isFlying()) {
			this.#playLoop("idleFlying", { speed: 1.25 });
			return;
		}
		this.#playLoop("standing");
	}

	#sampleReference(ref: BumblebeeScaleReference | null | undefined) {
		if (!ref) return { width: undefined, height: undefined };
		const resolved = typeof ref === "function" ? ref() : ref;
		if (!resolved) return { width: undefined, height: undefined };
		if (isHTMLElement(resolved)) {
			const rect = resolved.getBoundingClientRect();
			return { width: rect.width, height: rect.height };
		}
		return { width: resolved.x, height: resolved.y };
	}

	#refreshMoveBounds() {
		const { width, height } = this.#sampleReference(this.#lastScaleReference);
		this.#lastMoveBounds = {
			width: this.#normalizeDimension(width) ?? this.#lastMoveBounds.width,
			height: this.#normalizeDimension(height) ?? this.#lastMoveBounds.height
		};
		return this.#lastMoveBounds;
	}

	#normalizeDimension(value: number | undefined) {
		return typeof value === "number" && value > 2 ? value : undefined;
	}

	#normalizePositive(value: number | undefined, fallback: number) {
		return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : fallback;
	}

	#normalizeRange(value: number | undefined, min: number, max: number, fallback: number) {
		if (typeof value !== "number" || !Number.isFinite(value)) return fallback;
		return Math.max(min, Math.min(max, value));
	}

	#animationDestination(destination: PositionTarget): HTMLElement | Vector2 | Vector3 {
		if (
			isHTMLElement(destination) ||
			destination instanceof Vector2 ||
			destination instanceof Vector3
		) {
			return destination;
		}
		return new Vector2(destination.x * window.innerWidth, destination.y * window.innerHeight);
	}

	#worldDestination(
		destination: PositionTarget,
		scalePercentage: number,
		boundsWidth?: number,
		boundsHeight?: number
	) {
		if (destination instanceof Vector3 && isFiniteVector3(destination)) {
			return destination.clone();
		}
		const scene = this.runtime.surface.scene;
		const camera = scene.activeCamera;
		const canvas = scene.getEngine().getRenderingCanvas();
		if (!camera || !canvas) return this.node.position.clone();
		const screenDestination = this.#animationDestination(destination);
		if (screenDestination instanceof Vector3) {
			return isFiniteVector3(screenDestination)
				? screenDestination.clone()
				: this.node.position.clone();
		}
		const target = screenToWorld(
			this.node,
			screenDestination,
			camera,
			canvas,
			scalePercentage,
			boundsWidth,
			boundsHeight
		);
		return isFiniteVector3(target) ? target : this.node.position.clone();
	}

	#faceCamera() {
		const camera = this.runtime.surface.scene.activeCamera;
		if (!camera) return;
		this.node.lookAt(camera.position);
	}

	#moveCurveIntensity() {
		const snapshot = this.actorRef.getSnapshot();
		const isFlying =
			snapshot.matches({ visible: { stance: "idleFlying" } }) ||
			snapshot.matches({ visible: { stance: "activeFlying" } });
		return isFlying ? 0.5 : 0;
	}

	#enableAnchorMode() {
		if (this.#anchorModeEnabled) return;
		this.#anchorModeEnabled = true;
		this.#beforeRenderObserver = this.runtime.surface.scene.onBeforeRenderObservable.add(() =>
			this.#onBeforeRenderAnchoredLerp()
		);
		this.runtime.surface.scene.getEngine().onResizeObservable.add(this.#requestAnchorLerp);
		if (typeof window !== "undefined") {
			window.addEventListener("scroll", this.#requestAnchorLerp, { passive: true });
			window.addEventListener("resize", this.#requestAnchorLerpAfterLayoutChange, {
				passive: true
			});
			window.addEventListener("focus", this.#requestAnchorLerpAfterLayoutChange, { passive: true });
			window.visualViewport?.addEventListener("resize", this.#requestAnchorLerpAfterLayoutChange, {
				passive: true
			});
		}
	}

	#disableAnchorMode() {
		if (!this.#anchorModeEnabled) return;
		this.#anchorModeEnabled = false;
		const scene = this.runtime.surface.scene;
		if (this.#beforeRenderObserver) {
			scene.onBeforeRenderObservable.remove(this.#beforeRenderObserver);
			this.#beforeRenderObserver = null;
		}
		scene.getEngine().onResizeObservable.removeCallback(this.#requestAnchorLerp);
		if (typeof window !== "undefined") {
			window.removeEventListener("scroll", this.#requestAnchorLerp);
			window.removeEventListener("resize", this.#requestAnchorLerpAfterLayoutChange);
			window.removeEventListener("focus", this.#requestAnchorLerpAfterLayoutChange);
			window.visualViewport?.removeEventListener(
				"resize",
				this.#requestAnchorLerpAfterLayoutChange
			);
		}
		this.#clearAnchorResizeFrame();
		this.#clearAnchorTimeout();
		this.#clearAnchorThrottleTimeout();
		this.#anchorActive = false;
		this.#anchorLastFrameTime = 0;
		if (this.#anchorNeedsLanding) {
			void this.land();
			this.#anchorNeedsLanding = false;
		}
	}

	#requestAnchoredLerp() {
		if (!this.#anchorModeEnabled) return;
		const now = this.runtime.playbackNow();
		const elapsed = now - this.#anchorLastRequest;
		if (elapsed >= this.#anchorThrottleMs) {
			this.#anchorLastRequest = now;
			this.#scheduleAnchoredLerp();
			return;
		}
		if (this.#anchorThrottleTimeout) return;
		this.#anchorThrottleTimeout = this.runtime.setPlaybackTimeout(
			() => {
				this.#anchorThrottleTimeout = null;
				this.#anchorLastRequest = this.runtime.playbackNow();
				this.#scheduleAnchoredLerp();
			},
			Math.max(0, this.#anchorThrottleMs - elapsed)
		);
	}

	#scheduleAnchoredLerp(duration = this.#anchorSettleMs) {
		if (!this.#anchorModeEnabled || !this.#lastAnchorElement) return;
		if (!this.#anchorActive) {
			this.#anchorNeedsLanding = false;
			if (!this.#isFlying()) {
				void this.fly();
				this.#anchorNeedsLanding = true;
			}
		}
		this.#anchorActive = true;
		this.#clearAnchorTimeout();
		this.#anchorTimeout = this.runtime.setPlaybackTimeout(() => {
			this.#anchorActive = false;
			this.#anchorTimeout = null;
			if (this.#anchorNeedsLanding) {
				void this.land();
				this.#anchorNeedsLanding = false;
			}
		}, duration);
	}

	#onBeforeRenderAnchoredLerp() {
		if (
			!shouldApplyAnchoredLerp({
				anchorModeEnabled: this.#anchorModeEnabled,
				hasAnchor: Boolean(this.#lastAnchorElement),
				responsiveMoveActive: this.#responsiveAnchorMoveActive
			}) ||
			!this.#lastAnchorElement
		) {
			return;
		}
		const scene = this.runtime.surface.scene;
		const camera = scene.activeCamera;
		const canvas = scene.getEngine().getRenderingCanvas();
		if (!camera || !canvas) return;
		try {
			const now = this.runtime.playbackNow();
			const elapsedMs = this.#anchorLastFrameTime
				? now - this.#anchorLastFrameTime
				: ANCHOR_LERP_REFERENCE_FRAME_MS;
			this.#anchorLastFrameTime = now;
			const bounds = this.#refreshMoveBounds();
			const target = screenToWorld(
				this.node,
				this.#lastAnchorElement,
				camera,
				canvas,
				this.#lastMoveScale,
				bounds.width,
				bounds.height
			);
			this.node.position = Vector3.Lerp(
				this.node.position,
				target,
				resolveFrameRateIndependentLerpFactor(this.#anchorLerpFactor, elapsedMs)
			);
			this.node.lookAt(camera.position);
			this.#refreshActiveBubbleGeometry();
		} catch (error) {
			console.warn("Failed to lerp Bumblebee toward anchor:", error);
		}
	}

	async #playResponsiveAnchorMove(options: {
		destination: HTMLElement;
		scalePercentage: number;
		boundsWidth?: number;
		boundsHeight?: number;
		speed?: number;
		scrollFollower?: MoveScrollFollower | null;
	}) {
		const scene = this.runtime.surface.scene;
		const camera = scene.activeCamera;
		const canvas = scene.getEngine().getRenderingCanvas();
		if (!camera || !canvas) return;

		const startPosition = this.node.position.clone();
		const startRotation = nodeQuaternion(this.node);
		this.node.rotationQuaternion = startRotation.clone();
		let previousRotation = startRotation;
		const durationMs = 1000 / this.#normalizeRange(options.speed, 0.01, 100, 1);
		const startedAt = this.runtime.playbackNow();
		this.#responsiveAnchorMoveAbort?.abort();
		const abort = new AbortController();
		this.#responsiveAnchorMoveAbort = abort;
		this.#responsiveAnchorMoveActive = true;

		await new Promise<void>((resolve) => {
			let observer: Observer<Scene> | null = null;
			let settled = false;
			const cleanup = () => {
				if (observer) {
					scene.onBeforeRenderObservable.remove(observer);
					observer = null;
				}
				abort.signal.removeEventListener("abort", onAbort);
			};
			const finish = (applyFinalPose: boolean) => {
				if (settled) return;
				settled = true;
				cleanup();
				if (!applyFinalPose || this.#disposed || scene.isDisposed || this.node.isDisposed()) {
					resolve();
					return;
				}
				const bounds = this.#refreshMoveBounds();
				const finalTarget = this.#worldPositionForDestination(
					options.destination,
					options.scalePercentage,
					bounds.width,
					bounds.height
				);
				if (finalTarget) this.node.position.copyFrom(finalTarget);
				this.#facePoint(camera.position);
				this.#refreshActiveBubbleGeometry();
				resolve();
			};
			const onAbort = () => finish(false);
			abort.signal.addEventListener("abort", onAbort, { once: true });

			observer = scene.onBeforeRenderObservable.add(() => {
				if (abort.signal.aborted || this.#disposed || scene.isDisposed || this.node.isDisposed()) {
					finish(false);
					return;
				}
				const now = this.runtime.playbackNow();
				const progress = Math.min(1, Math.max(0, (now - startedAt) / durationMs));
				options.scrollFollower?.update(progress);
				const easedProgress = this.#easeInOutCubic(progress);
				const bounds = this.#refreshMoveBounds();
				const target = this.#worldPositionForDestination(
					options.destination,
					options.scalePercentage,
					bounds.width,
					bounds.height
				);
				if (!target) {
					finish(true);
					return;
				}
				this.node.position = Vector3.Lerp(startPosition, target, easedProgress);
				const pathRotation = this.#rotationFacingPoint(target, previousRotation);
				let nextRotation: Quaternion;
				if (progress < 0.2) {
					nextRotation = Quaternion.Slerp(
						startRotation,
						alignQuaternionSign(pathRotation, startRotation),
						progress / 0.2
					);
				} else if (progress > 0.82) {
					const cameraRotation = this.#rotationFacingPoint(camera.position, pathRotation);
					nextRotation = Quaternion.Slerp(
						pathRotation,
						alignQuaternionSign(cameraRotation, pathRotation),
						(progress - 0.82) / 0.18
					);
				} else {
					nextRotation = pathRotation;
				}
				nextRotation = alignQuaternionSign(nextRotation, previousRotation);
				this.node.rotationQuaternion = nextRotation;
				previousRotation = nextRotation;
				this.#refreshActiveBubbleGeometry();
				if (progress >= 1) finish(true);
			});
		}).finally(() => {
			if (this.#responsiveAnchorMoveAbort === abort) {
				this.#responsiveAnchorMoveAbort = null;
			}
			this.#responsiveAnchorMoveActive = false;
		});
	}

	#worldPositionForDestination(
		destination: HTMLElement | Vector2 | Vector3,
		scalePercentage: number,
		boundsWidth?: number,
		boundsHeight?: number
	) {
		if (destination instanceof Vector3) return destination;
		const scene = this.runtime.surface.scene;
		const camera = scene.activeCamera;
		const canvas = scene.getEngine().getRenderingCanvas();
		if (!camera || !canvas) return null;
		const target = screenToWorld(
			this.node,
			destination,
			camera,
			canvas,
			scalePercentage,
			boundsWidth,
			boundsHeight
		);
		return Number.isFinite(target.x) && Number.isFinite(target.y) && Number.isFinite(target.z)
			? target
			: null;
	}

	#rotationFacingPoint(point: Vector3, fallback = nodeQuaternion(this.node)) {
		const camera = this.runtime.surface.scene.activeCamera;
		if (!camera) return fallback.clone();
		if (point.subtract(this.node.position).lengthSquared() < 1e-10) return fallback.clone();
		const rotation = Quaternion.FromRotationMatrix(
			Matrix.LookAtLH(this.node.position, point, camera.upVector).invert()
		);
		return alignQuaternionSign(rotation, fallback);
	}

	#facePoint(point: Vector3) {
		const current = nodeQuaternion(this.node);
		this.node.rotationQuaternion = this.#rotationFacingPoint(point, current);
	}

	#easeInOutCubic(value: number) {
		return value < 0.5 ? 4 * value * value * value : 1 - Math.pow(-2 * value + 2, 3) / 2;
	}

	#trackBubbleGeometryDuringMove() {
		return this.runtime.surface.scene.onBeforeRenderObservable.add(() =>
			this.#refreshActiveBubbleGeometry()
		);
	}

	#removeBubbleRefreshObserver(observer: Observer<Scene> | null) {
		if (!observer) return;
		this.runtime.surface.scene.onBeforeRenderObservable.remove(observer);
	}

	#refreshActiveBubbleGeometry() {
		this.runtime.refreshActorBubbleGeometry(this);
	}

	#clearAnchorTimeout() {
		if (!this.#anchorTimeout) return;
		this.runtime.clearPlaybackTimeout(this.#anchorTimeout);
		this.#anchorTimeout = null;
	}

	#clearAnchorThrottleTimeout() {
		if (!this.#anchorThrottleTimeout) return;
		this.runtime.clearPlaybackTimeout(this.#anchorThrottleTimeout);
		this.#anchorThrottleTimeout = null;
	}

	#clearAnchorResizeFrame() {
		if (this.#anchorResizeFrame === null) return;
		cancelAnimationFrame(this.#anchorResizeFrame);
		this.#anchorResizeFrame = null;
	}

	#isFlying() {
		const snapshot = this.actorRef.getSnapshot();
		return (
			snapshot.matches({ visible: { stance: "idleFlying" } }) ||
			snapshot.matches({ visible: { stance: "activeFlying" } })
		);
	}

	#isVisible() {
		const value = this.actorRef.getSnapshot().value;
		return typeof value === "object" && value !== null && "visible" in value;
	}
}
