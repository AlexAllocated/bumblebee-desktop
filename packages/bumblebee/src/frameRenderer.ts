import "./style.css";
import { Quaternion, Vector2, Vector3 } from "@babylonjs/core/Maths/math.vector";
import type { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import {
	createFrameBubbleRenderer,
	createPoseFromSubject,
	type FrameBubbleOptions
} from "@hivetech/speech-bubbles";
import { createScene } from "./bumblebee/createScene";
import { loadModel as loadBee } from "./bumblebee/loadModel";
import { loadModel as loadPuppet } from "./puppet/loadModel";
import {
	getPuppetNameplateLocalAnchor,
	PUPPET_STICK_BOTTOM_Y,
	PUPPET_STICK_TOP_Y,
	type PuppetVisibleArtBounds
} from "./puppet/geometry";
import {
	PUPPET_TALK_HZ,
	puppetSpeechRotation,
	puppetTalkIntensityForFrame,
	speechOcclusionForFrame
} from "./puppet/speechMotion";
import { SpeechMotion } from "./utils/speechMotion";
import { BUMBLEBEE_TALK_WEIGHT } from "./bumblebee/speechMotion";
import { FrameAnimationSampler } from "./frameAnimation";
import { createFrameMovement } from "./frameMovement";
import { setSceneRandomSeed } from "./utils/sceneRandom";
import { screenToWorld } from "./utils/screenToWorld";
import { captureNodeLocalBounds, projectNodeLocalBoundsToScreenRect } from "./utils/screenRect";
import { Nameplates, type NameplatePlatform } from "./overlay/nameplates";
import { sampleBumblebeeTimeline, speechAssetDurationMs, type BumblebeeTimeline } from "./timeline";

type FrameActorBase = {
	id: string;
	timeline: BumblebeeTimeline;
	bubble?: Partial<Pick<FrameBubbleOptions, "style" | "scale" | "maxWidthPx" | "maxHeightPx">> & {
		angle?: number;
		tailLength?: number;
	};
};
export type FrameActor = FrameActorBase &
	(
		| { kind: "bumblebee"; modelUrl: string }
		| {
				kind: "puppet";
				imageUrl: string;
				name: string;
				platform?: NameplatePlatform;
				stickColor?: string;
				/** Use the live puppet's 180° Y-axis facing flip; text decorations stay upright. */
				facingLeft?: boolean;
		  }
	);
export type FrameRendererOptions = {
	container: HTMLElement;
	width: number;
	height: number;
	durationMs: number;
	actors: FrameActor[];
	seed?: number;
	/** Presentation size multiplier; position and rotation always follow the artwork. */
	nameplateScale?: number;
};

/** Offline presentation surface. No bot connection, live actor loop, or audio playback. */
export const createFrameRenderer = (options: FrameRendererOptions) => {
	if (
		![options.width, options.height, options.durationMs].every((n) => Number.isFinite(n) && n > 0)
	) {
		throw new Error("Frame renderer dimensions and duration must be positive and finite.");
	}
	if (new Set(options.actors.map((actor) => actor.id)).size !== options.actors.length)
		throw new Error("Frame actor IDs must be unique.");
	for (const actor of options.actors) {
		for (const action of actor.timeline.actions) {
			if (
				action.type === "say" &&
				(!action.speech.audioSrc ||
					!action.speech.timeline?.words.length ||
					speechAssetDurationMs(action.speech) <= 0)
			) {
				throw new Error(`Speech ${action.speech.id} is not prepared.`);
			}
		}
	}
	const scene = createScene({
		container: options.container,
		manualRender: true,
		performanceProfile: "normal",
		zIndex: 1
	});
	const engine = scene.getEngine();
	const canvas = engine.getRenderingCanvas()!;
	canvas.style.visibility = "hidden";
	engine.setHardwareScalingLevel(1);
	engine.setSize(options.width, options.height);
	scene.animationsEnabled = false;
	setSceneRandomSeed(scene, options.seed ?? 1);
	const bubbleContainer = document.createElement("div");
	bubbleContainer.style.cssText = "position:absolute;inset:0;pointer-events:none;z-index:3";
	options.container.append(bubbleContainer);
	const nameplates = new Nameplates({ container: bubbleContainer, zIndex: 2, scale: 1.2 });
	let disposed = false;
	let initialized = false;
	type LoadedActor = {
		definition: FrameActor;
		node: TransformNode;
		contentNode?: TransformNode;
		animations?: FrameAnimationSampler;
		movement?: ReturnType<typeof createFrameMovement>;
		bounds: ReturnType<typeof captureNodeLocalBounds>;
		stanceBounds: Partial<
			Record<"flying" | "standing", NonNullable<ReturnType<typeof captureNodeLocalBounds>>>
		>;
		bubbles: ReturnType<typeof createFrameBubbleRenderer>[];
		speechMotion?: Array<{ rotation: number; occlusionDelta: number }>;
		beeSpeechMotion?: Array<{ elapsedMs: number; weight: number }>;
	};
	const actors: LoadedActor[] = [];
	const dispose = () => {
		if (disposed) return;
		disposed = true;
		for (const actor of actors) {
			actor.movement?.dispose();
			for (const bubble of actor.bubbles) bubble.dispose();
		}
		nameplates.dispose();
		bubbleContainer.remove();
		scene.dispose();
		engine.dispose();
		canvas.remove();
	};
	const seek = (timeMs: number) => {
		if (disposed) throw new Error("Frame renderer has been disposed.");
		if (!initialized) throw new Error("Await frame renderer.ready before seeking.");
		if (!Number.isFinite(timeMs)) throw new Error("Frame time must be finite.");
		const t = Math.max(0, Math.min(options.durationMs, timeMs));
		const states = [];
		for (const actor of actors) {
			const state = sampleBumblebeeTimeline(actor.definition.timeline, t);
			states.push({ id: actor.definition.id, ...state });
			const movement = actor.movement?.seek(t);
			const puppetMotion =
				actor.speechMotion?.[Math.min(actor.speechMotion.length - 1, Math.round((t * 120) / 1000))];
			const beeMotion =
				actor.beeSpeechMotion?.[
					Math.min(actor.beeSpeechMotion.length - 1, Math.round((t * 120) / 1000))
				];
			actor.animations?.reset();
			if (actor.animations) {
				actor.animations.apply(state.stance === "flying" ? "idleFlying" : "standing", t, {
					loop: true
				});
				if (movement?.travelWeight)
					actor.animations.apply(
						state.stance === "flying" ? "activeFlying" : "walking",
						movement.elapsedMs * 1.25,
						{
							loop: true,
							weight: movement.travelWeight
						}
					);
				if (beeMotion?.weight)
					actor.animations.apply(
						state.stance === "flying" ? "talking" : "standingTalking",
						beeMotion.elapsedMs,
						{ loop: true, weight: beeMotion.weight }
					);
				for (const emote of state.activeEmotes)
					actor.animations.apply(emote.name, t - emote.startMs, { durationMs: emote.durationMs });
			}
			actor.node.setEnabled(state.visible && t < options.durationMs);
			if (movement) actor.node.rotationQuaternion = movement.rotation;
			const localBounds =
				actor.stanceBounds[state.stance === "flying" ? "flying" : "standing"] ?? actor.bounds;
			if (localBounds) {
				actor.node.metadata.localDims = {
					w: localBounds.maximum.x - localBounds.minimum.x,
					h: localBounds.maximum.y - localBounds.minimum.y
				};
			}
			actor.node.position.copyFrom(
				movement?.center ??
					screenToWorld(
						actor.node,
						new Vector2(
							state.pose.position.x * options.width,
							(state.pose.position.y + (puppetMotion?.occlusionDelta ?? 0) * state.pose.scale) *
								options.height
						),
						scene.activeCamera!,
						canvas,
						state.pose.scale
					)
			);
			if (localBounds) {
				actor.node.computeWorldMatrix(true);
				actor.node.position.subtractInPlace(
					Vector3.TransformNormal(
						Vector3.Center(localBounds.minimum, localBounds.maximum),
						actor.node.getWorldMatrix()
					)
				);
			}
			if (actor.contentNode) {
				actor.contentNode.rotation.z = puppetMotion?.rotation ?? 0;
				actor.contentNode.computeWorldMatrix(true);
			}
			actor.node.computeWorldMatrix(true);
			const bounds = localBounds
				? projectNodeLocalBoundsToScreenRect(actor.node, localBounds)
				: null;
			if (bounds) {
				const rect = bubbleContainer.getBoundingClientRect();
				bounds.left -= rect.left;
				bounds.right -= rect.left;
				bounds.top -= rect.top;
				bounds.bottom -= rect.top;
			}
			const center = bounds
				? { x: (bounds.left + bounds.right) / 2, y: (bounds.top + bounds.bottom) / 2 }
				: { x: state.pose.position.x * options.width, y: state.pose.position.y * options.height };
			// Puppet speech points at the art, not the center of its long handle.
			if (bounds && actor.contentNode) center.y = bounds.top + (bounds.bottom - bounds.top) * 0.23;
			// Flying envelopes include vertical bob travel. Using their entire height
			// puts captions far above the character (and into composition headlines).
			const radius = bounds
				? actor.contentNode
					? bounds.width * 0.42
					: Math.min(bounds.width, bounds.height) * 0.5
				: 80;
			const pose = createPoseFromSubject({
				center,
				radius,
				anchorAngle: actor.definition.bubble?.angle ?? Math.PI / 2,
				tailLength: actor.definition.bubble?.tailLength ?? 50
			});
			for (const bubble of actor.bubbles) {
				bubble.seek(t, pose);
				if (!state.visible || t >= options.durationMs) bubble.element.style.visibility = "hidden";
			}
			if (actor.definition.kind === "puppet") {
				const art = actor.node.metadata.visibleArtBounds as PuppetVisibleArtBounds;
				const local = getPuppetNameplateLocalAnchor(art);
				const point = new Vector3(local.x, local.y, 0);
				// Project the stable artwork anchor, never the entire handle's bottom
				// or a screen-edge clamp. This is the same anchor used by live puppets.
				const target = actor.contentNode ?? actor.node;
				const anchor = projectNodeLocalBoundsToScreenRect(target, {
					minimum: point,
					maximum: point
				});
				const project = (y: number) =>
					projectNodeLocalBoundsToScreenRect(target, {
						minimum: new Vector3(0, y, 0),
						maximum: new Vector3(0, y, 0)
					});
				const bottom = project(PUPPET_STICK_BOTTOM_Y),
					top = project(PUPPET_STICK_TOP_Y);
				const rotationDeg =
					bottom && top
						? (Math.atan2(top.top - bottom.top, top.left - bottom.left) * 180) / Math.PI + 90
						: 0;
				if (state.visible && t < options.durationMs && anchor)
					nameplates.show(
						actor.definition.id,
						{ text: actor.definition.name, platform: actor.definition.platform },
						{
							// Nameplates converts viewport coordinates into its container.
							x: anchor.left,
							y: anchor.top,
							scale: options.nameplateScale ?? 1.6,
							constrainToViewport: false,
							rotationDeg
						}
					);
				else nameplates.hide(actor.definition.id);
			}
		}
		nameplates.seek(t);
		engine.beginFrame();
		scene.render();
		engine.endFrame();
		canvas.style.visibility = "visible";
		return states;
	};
	const ready = (async () => {
		try {
			// Sequential asset construction keeps seeded procedural textures stable across runs.
			for (const definition of options.actors) {
				if (disposed) throw new Error("Frame renderer disposed while loading.");
				const loaded =
					definition.kind === "bumblebee"
						? await loadBee({ scene, modelUrl: definition.modelUrl })
						: await loadPuppet({
								scene,
								puppetId: definition.id,
								imageUrl: definition.imageUrl,
								stickColor: definition.stickColor,
								renderMode: "flat",
								textureResolution: 2048
							});
				if (disposed) {
					loaded.node.dispose();
					throw new Error("Frame renderer disposed while loading.");
				}
				loaded.node.setEnabled(true);
				if (definition.kind === "puppet" && definition.facingLeft)
					loaded.node.rotationQuaternion = Quaternion.RotationAxis(Vector3.Up(), Math.PI);
				const bubbles = definition.timeline.actions
					.filter((action) => action.type === "say" && action.bubbles !== false)
					.map((action) => {
						if (action.type !== "say") throw new Error("Expected speech action");
						return createFrameBubbleRenderer({
							id: `${definition.id}:${action.speech.id}`,
							target: bubbleContainer,
							text: action.speech.text,
							pose: createPoseFromSubject({ center: { x: 0, y: 0 }, radius: 1 }),
							startMs: action.startMs,
							durationMs: speechAssetDurationMs(action.speech),
							timeline: action.speech.timeline,
							viewport: { width: options.width, height: options.height },
							maxWidthPx: 780,
							maxHeightPx: 430,
							maxWidthPercent: 75,
							scale: 1.6,
							...definition.bubble
						});
					});
				const animations =
					definition.kind === "bumblebee"
						? new FrameAnimationSampler(
								loaded.animationGroups as ConstructorParameters<typeof FrameAnimationSampler>[0]
							)
						: undefined;
				const stanceBounds: LoadedActor["stanceBounds"] = {};
				if (animations) {
					// Capture a stable movement envelope per stance, including the rig's
					// flying translation. Position and bubble anchors never follow a bone.
					for (const stance of ["standing", "flying"] as const) {
						const name = stance === "flying" ? "idleFlying" : "standing";
						const group = animations.groups.find((group) => group.name === name)!;
						const duration =
							((group.to - group.from) / group.targetedAnimations[0].animation.framePerSecond) *
							1000;
						for (let sample = 0; sample <= 32; sample++) {
							animations.reset();
							animations.apply(name, (duration * sample) / 32);
							scene.render();
							for (const mesh of loaded.node.getChildMeshes())
								mesh.refreshBoundingInfo({ applySkeleton: true, applyMorph: true });
							const bounds = captureNodeLocalBounds(loaded.node);
							if (!bounds) throw new Error(`Cannot measure ${definition.id} in ${stance} stance.`);
							const previous = stanceBounds[stance];
							stanceBounds[stance] = previous
								? {
										minimum: Vector3.Minimize(previous.minimum, bounds.minimum),
										maximum: Vector3.Maximize(previous.maximum, bounds.maximum)
									}
								: bounds;
						}
					}
					animations.reset();
				}
				const movement = animations
					? createFrameMovement(loaded.node, definition.timeline, (pose, timeMs) => {
							const stance = sampleBumblebeeTimeline(definition.timeline, timeMs).stance;
							const bounds = stanceBounds[stance];
							if (bounds)
								loaded.node.metadata.localDims = {
									w: bounds.maximum.x - bounds.minimum.x,
									h: bounds.maximum.y - bounds.minimum.y
								};
							return screenToWorld(
								loaded.node,
								new Vector2(pose.position.x * options.width, pose.position.y * options.height),
								scene.activeCamera!,
								canvas,
								pose.scale
							);
						})
					: undefined;
				// Sample the live speech envelope at a fixed rate once. Seeking backward or
				// exporting frames out of order must not advance a live animation clock.
				const speechMotion: LoadedActor["speechMotion"] = [];
				const beeSpeechMotion: LoadedActor["beeSpeechMotion"] = [];
				if (definition.timeline.actions.some((action) => action.type === "say")) {
					const motion = new SpeechMotion();
					let phase = 0,
						animationElapsedMs = 0,
						previousSpeech = "";
					for (let frame = 0; frame <= Math.ceil((options.durationMs * 120) / 1000); frame++) {
						const time = (frame * 1000) / 120;
						const state = sampleBumblebeeTimeline(definition.timeline, time);
						const speech = state.activeSpeech;
						if (!speech) {
							motion.reset();
							phase = 0;
							animationElapsedMs = 0;
							previousSpeech = "";
							speechMotion.push({ rotation: 0, occlusionDelta: 0 });
							beeSpeechMotion.push({ elapsedMs: 0, weight: 0 });
							continue;
						}
						if (previousSpeech !== speech.asset.id) {
							motion.reset();
							phase = 0;
							animationElapsedMs = 0;
						}
						previousSpeech = speech.asset.id;
						const envelope = motion.updateViseme(speech.currentViseme?.visemeId ?? 0, time);
						// Match live Bumblebee: smooth viseme-driven speed with steady
						// additive strength. Raw mouth shapes must never jerk the body.
						animationElapsedMs += (envelope.talkSpeed * 1000) / 120;
						const blend = Math.max(
							0,
							Math.min(1, speech.elapsedMs / 125, (speech.durationMs - speech.elapsedMs) / 125)
						);
						beeSpeechMotion.push({
							elapsedMs: animationElapsedMs,
							weight: BUMBLEBEE_TALK_WEIGHT * blend
						});
						phase += (PUPPET_TALK_HZ * Math.PI * 2 * envelope.talkSpeed) / 120;
						speechMotion.push({
							rotation: puppetSpeechRotation(phase, puppetTalkIntensityForFrame(envelope)),
							occlusionDelta: speechOcclusionForFrame(0.25, envelope.normalizedLevel) - 0.25
						});
					}
				}
				actors.push({
					definition,
					node: loaded.node,
					bounds: captureNodeLocalBounds(loaded.node),
					stanceBounds,
					contentNode: "contentNode" in loaded ? loaded.contentNode : undefined,
					animations,
					movement,
					speechMotion: definition.kind === "puppet" ? speechMotion : undefined,
					beeSpeechMotion: definition.kind === "bumblebee" ? beeSpeechMotion : undefined,
					bubbles
				});
			}
			await document.fonts.ready;
			await scene.whenReadyAsync();
			if (disposed) throw new Error("Frame renderer disposed while loading.");
			initialized = true;
			seek(0);
		} catch (error) {
			dispose();
			throw error;
		}
	})();
	return { ready, seek, dispose };
};
