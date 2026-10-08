import { PUPPET_TALK_HZ, puppetSpeechRotation } from "./speechMotion";
import z from "zod";
import type { AbstractMesh } from "@babylonjs/core/Meshes/abstractMesh";
import { Animation } from "@babylonjs/core/Animations/animation";
import { AnimationEvent } from "@babylonjs/core/Animations/animationEvent";
import { AnimationGroup } from "@babylonjs/core/Animations/animationGroup";
import { CubicEase, EasingFunction } from "@babylonjs/core/Animations/easing";
import { Color3 } from "@babylonjs/core/Maths/math.color";
import { Matrix, Quaternion, Vector3 } from "@babylonjs/core/Maths/math.vector";
import type { Material } from "@babylonjs/core/Materials/material";
import { PBRMaterial } from "@babylonjs/core/Materials/PBR/pbrMaterial";
import { Scene } from "@babylonjs/core/scene";
import { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import { Viewport } from "@babylonjs/core/Maths/math.viewport";
import type { Observer } from "@babylonjs/core/Misc/observable";
import "@babylonjs/core/Rendering/edgesRenderer";
import { loadModel } from "./loadModel";
import { createShowAnimation, getShowNaturalRollQuaternion } from "./animations/show";
import { playAnimation, playDisposableAnimation } from "../utils/playAnimation";
import { createHideAnimation } from "./animations/hide";
import { createMoveToSpawnAnimation } from "./animations/moveToSpawn";
import { PuppetPositionManager, type PuppetPosition } from "./positionManager";
import { resolveCanvasViewportSize } from "../utils/screenToWorld";
import { resolveEffectivePuppetTextureResolution } from "../utils/babylonPerformance";
import type { PuppetRenderableMetadata } from "../types";
import {
	getPuppetNameplateLocalAnchor,
	PUPPET_STICK_BOTTOM_Y,
	PUPPET_STICK_TOP_Y
} from "./geometry";
import { playbackNow, playbackPaused } from "../overlay/playbackClock";

const OptionsSchema = z.object({
	puppetId: z.string(),
	imageUrl: z.string().optional(),
	imageMask: z.enum(["none", "circle"]).optional(),
	scene: z.instanceof(Scene),
	puppetPosition: z
		.object({
			horizontalPercent: z.number()
		})
		.optional(),
	scalePercentage: z.number().optional(),
	occlusionPercentage: z.number().optional(),
	stickColor: z.string().optional(),
	textureResolution: z.number().int().min(256).max(4096).optional(),
	renderMode: z.enum(["full", "half", "flat"]).optional().default("flat"),
	appearance: z.enum(["normal", "ghost"]).optional().default("normal"),
	renderMetadata: z.custom<PuppetRenderableMetadata | null>().optional()
});
type Options = z.input<typeof OptionsSchema>;

type GhostDepthMaterial = Material & {
	forceDepthWrite?: boolean;
	alphaCutOff?: number;
	transparencyMode?: number;
	needDepthPrePass?: boolean;
};

type GhostDepthMetadata = {
	ghostDepth?: {
		forceDepthWrite: boolean;
		alphaCutOff?: number;
		transparencyMode?: number;
		needDepthPrePass: boolean;
	} | null;
};
type PuppetMeshMetadata = GhostDepthMetadata & {
	bumblebeePuppetPart?: "art-plane" | "art-fin";
	bumblebeePuppetArtPlaneEdge?: "front" | "back" | null;
};

const nowMs = () =>
	typeof performance !== "undefined" && typeof performance.now === "function"
		? performance.now()
		: Date.now();
const NAMEPLATE_REFERENCE_STICK_PX = 260;
const NAMEPLATE_SCALE_MULTIPLIER = 0.85;
const NAMEPLATE_MIN_SCALE = 0.72;
const NAMEPLATE_MAX_SCALE = 1.35;

const emitPuppetRenderMetric = (detail: {
	puppetId: string;
	imageUrl?: string;
	renderMode: "full" | "half" | "flat";
	textureResolution?: number;
	usedRenderMetadata: boolean;
	durationMs: number;
	meshCount: number;
}) => {
	if (typeof window === "undefined") return;
	window.dispatchEvent(new CustomEvent("bumblebee:puppet-render-metric", { detail }));
	try {
		if (window.localStorage?.getItem("bumblebee:puppetMetrics") === "1") {
			console.debug("[puppet-render]", detail);
		}
	} catch {
		// Storage can be unavailable in hardened browser contexts; metrics still emit as events.
	}
};

const createPuppet = async (options: Options) => {
	const {
		puppetId,
		imageUrl,
		imageMask,
		scene,
		puppetPosition,
		scalePercentage,
		occlusionPercentage,
		stickColor,
		textureResolution,
		renderMode,
		appearance,
		renderMetadata
	} = OptionsSchema.parse(options);

	const startedAt = nowMs();
	const { node, contentNode, ensureStickDimOverlay } = await loadModel({
		puppetId,
		imageUrl,
		imageMask,
		scene,
		stickColor,
		textureResolution,
		renderMode,
		renderMetadata
	});
	emitPuppetRenderMetric({
		puppetId,
		imageUrl,
		renderMode,
		textureResolution,
		usedRenderMetadata: Boolean(renderMetadata),
		durationMs: Math.round(nowMs() - startedAt),
		meshCount: node.getChildMeshes(false).length
	});

	if (typeof stickColor === "string" && stickColor.trim().length) {
		node.metadata = { ...(node.metadata || {}), stickColor: stickColor.trim() };
	}
	node.metadata = {
		...(node.metadata || {}),
		appearance,
		effectiveTextureResolution: resolveEffectivePuppetTextureResolution({
			renderMode,
			textureResolution
		}),
		imageMask,
		imageUrl,
		renderMode,
		textureResolution
	};

	let talkingRestRotationZ: number | null = null;
	let talkingObserver: Observer<Scene> | null = null;
	let talkingReturnAnimation: ReturnType<Scene["beginDirectAnimation"]> | null = null;
	let organicTiltAnimation: ReturnType<Scene["beginDirectAnimation"]> | null = null;
	let organicTiltSeq = 0;
	let talkingPhase = 0;
	let currentTalkingSpeed = 1;
	let currentTalkingIntensity = 1;
	const talkTarget = contentNode ?? node;
	const positionManager = new PuppetPositionManager(node, scene);
	if (typeof occlusionPercentage === "number") {
		positionManager.setOcclusionRatio(occlusionPercentage);
	}
	const flipQuaternion = Quaternion.RotationAxis(Vector3.Up(), Math.PI);

	let facingLeft = !!node.metadata?.facingLeft;
	const setFacingState = (value: boolean) => {
		facingLeft = value;
		node.metadata = { ...(node.metadata || {}), facingLeft };
	};
	setFacingState(facingLeft);

	const computeTargetFacingLeft = () => positionManager.getXPercent() >= 0.5;

	const setFacingImmediate = (targetFacingLeft: boolean) => {
		if (targetFacingLeft === facingLeft) return;
		node.rotationQuaternion = (node.rotationQuaternion ?? Quaternion.Identity()).multiply(
			flipQuaternion
		);
		setFacingState(targetFacingLeft);
	};

	const getFacingQuaternion = () => (facingLeft ? flipQuaternion.clone() : Quaternion.Identity());

	const getOrganicTiltTarget = (opts?: { disableTilt?: boolean }) =>
		getShowNaturalRollQuaternion(getFacingQuaternion(), positionManager, {
			disableRoll: !!opts?.disableTilt
		});

	const animateRotationQuaternion = (target: Quaternion, durationMs: number) => {
		const seq = ++organicTiltSeq;
		organicTiltAnimation?.stop();
		organicTiltAnimation = null;
		const start = node.rotationQuaternion
			? node.rotationQuaternion.clone()
			: Quaternion.FromEulerAngles(node.rotation.x, node.rotation.y, node.rotation.z);
		node.rotationQuaternion = start.clone();
		const frames = Math.max(1, Math.round((durationMs / 1000) * 60));
		const anim = new Animation(
			"puppetOrganicTilt",
			"rotationQuaternion",
			60,
			Animation.ANIMATIONTYPE_QUATERNION,
			Animation.ANIMATIONLOOPMODE_CONSTANT
		);
		anim.setKeys([
			{ frame: 0, value: start },
			{ frame: frames, value: target }
		]);
		const ease = new CubicEase();
		ease.setEasingMode(EasingFunction.EASINGMODE_EASEINOUT);
		anim.setEasingFunction(ease);
		const animatable = scene.beginDirectAnimation(node, [anim], 0, frames, false, 1);
		organicTiltAnimation = animatable;
		return new Promise<void>((resolve) => {
			animatable.onAnimationEndObservable.addOnce(() => {
				if (seq !== organicTiltSeq) {
					resolve();
					return;
				}
				node.rotationQuaternion = target.clone();
				if (organicTiltAnimation === animatable) {
					organicTiltAnimation = null;
				}
				resolve();
			});
		});
	};

	const resetRotationToFacing = () => {
		organicTiltSeq += 1;
		organicTiltAnimation?.stop();
		organicTiltAnimation = null;
		node.rotationQuaternion = getFacingQuaternion();
	};

	const applyOrganicTilt = (opts?: {
		disableTilt?: boolean;
		animate?: boolean;
		durationMs?: number;
	}) => {
		const target = getOrganicTiltTarget(opts);
		if (opts?.animate && node.isEnabled()) {
			return animateRotationQuaternion(target, opts.durationMs ?? 180);
		}
		organicTiltSeq += 1;
		organicTiltAnimation?.stop();
		organicTiltAnimation = null;
		node.rotationQuaternion = target;
	};

	const cancelOrganicTiltTransition = () => {
		organicTiltSeq += 1;
		organicTiltAnimation?.stop();
		organicTiltAnimation = null;
	};

	const addFacingFlipToGroup = (
		group: AnimationGroup,
		options: {
			durationMs: number;
			targetFacingLeft: boolean;
			totalFrames?: number;
			flipFrame?: number | null;
		}
	) => {
		const { durationMs, targetFacingLeft, totalFrames, flipFrame } = options;
		if (targetFacingLeft === facingLeft) return false;
		const baseQ = node.rotationQuaternion
			? node.rotationQuaternion.clone()
			: Quaternion.FromEulerAngles(node.rotation.x, node.rotation.y, node.rotation.z);
		node.rotationQuaternion = baseQ.clone();
		const startQ = baseQ.clone();
		const endQ = baseQ.multiply(flipQuaternion);
		const fps = 60;
		const frames = totalFrames ?? Math.max(1, Math.round((durationMs / 1000) * fps));
		let startFrame = 0;
		let endFrame = frames;
		if (typeof flipFrame === "number" && Number.isFinite(flipFrame)) {
			const flipFrames = Math.min(frames, Math.max(6, Math.round(frames * 0.12)));
			startFrame = Math.min(Math.max(flipFrame, 0), frames);
			endFrame = Math.min(frames, startFrame + flipFrames);
		}
		const rotAnim = new Animation(
			"puppetFlip",
			"rotationQuaternion",
			fps,
			Animation.ANIMATIONTYPE_QUATERNION,
			Animation.ANIMATIONLOOPMODE_CONSTANT
		);
		rotAnim.setKeys([
			{ frame: 0, value: startQ.clone() },
			{ frame: startFrame, value: startQ.clone() },
			{ frame: endFrame, value: endQ.clone() },
			{ frame: frames, value: endQ.clone() }
		]);
		const ease = new CubicEase();
		ease.setEasingMode(EasingFunction.EASINGMODE_EASEINOUT);
		rotAnim.setEasingFunction(ease);
		group.addTargetedAnimation(rotAnim, node);
		group.onAnimationGroupEndObservable.add(() => {
			setFacingState(targetFacingLeft);
		});
		return true;
	};

	const alignFacingWithPosition = async (opts?: { animate?: boolean; durationMs?: number }) => {
		const targetFacingLeft = computeTargetFacingLeft();
		if (targetFacingLeft === facingLeft) return;
		const shouldAnimate = !!opts?.animate && node.isEnabled();
		if (shouldAnimate && moveInFlight > 0) return;
		if (!shouldAnimate) {
			setFacingImmediate(targetFacingLeft);
			return;
		}
		const durationMs = opts?.durationMs ?? 900;
		const group = new AnimationGroup("puppetFlipSolo", scene);
		const added = addFacingFlipToGroup(group, { durationMs, targetFacingLeft });
		if (!added) {
			group.dispose();
			return;
		}
		await playDisposableAnimation({
			animationGroup: group,
			scene,
			blendMode: "none",
			resetToInitialOnStop: false
		});
	};

	const playFacingFlip = (targetFacingLeft: boolean) => {
		if (targetFacingLeft === facingLeft) return;
		const fps = 60;
		const flipDurationMs = 220;
		const frames = Math.max(4, Math.round((flipDurationMs / 1000) * fps));
		const startQ = node.rotationQuaternion
			? node.rotationQuaternion.clone()
			: Quaternion.FromEulerAngles(node.rotation.x, node.rotation.y, node.rotation.z);
		node.rotationQuaternion = startQ.clone();
		const endQ = startQ.multiply(flipQuaternion);
		const rotAnim = new Animation(
			"puppetFlipOnce",
			"rotationQuaternion",
			fps,
			Animation.ANIMATIONTYPE_QUATERNION,
			Animation.ANIMATIONLOOPMODE_CONSTANT
		);
		rotAnim.setKeys([
			{ frame: 0, value: startQ.clone() },
			{ frame: frames, value: endQ.clone() }
		]);
		const ease = new CubicEase();
		ease.setEasingMode(EasingFunction.EASINGMODE_EASEINOUT);
		rotAnim.setEasingFunction(ease);
		const animatable = scene.beginDirectAnimation(node, [rotAnim], 0, frames, false, 1.0);
		animatable.onAnimationEndObservable.addOnce(() => {
			setFacingState(targetFacingLeft);
		});
	};

	// Apply initial spawn and scale at creation so later show() respects updates
	if (puppetPosition) {
		positionManager.setPuppetPosition(puppetPosition);
	}
	if (typeof scalePercentage === "number") {
		positionManager.setScalePercentage(scalePercentage);
	}

	setFacingImmediate(computeTargetFacingLeft());

	let moveSeq = 0;
	let moveInFlight = 0;
	let activeMoveAbort: AbortController | null = null;
	let ghostModeEnabled = appearance === "ghost";
	let dimmedModeEnabled = false;
	let puppetOpacity = 1;
	let dimmedProgress = 0;
	let dimmedAnimationFrame: number | null = null;
	const ART_DIM_COLOR_MULTIPLIER = 0.48;
	const CIRCLE_DIM_OVERLAY_ALPHA = 0.48;
	const STICK_DIM_ALPHA = 0.45;
	const STICK_DIM_VISIBILITY_MULTIPLIER = 0.38;
	const DIM_ANIMATION_MS = 220;

	const hasNamedAncestor = (mesh: { name?: string; parent?: unknown }, prefix: string) => {
		let current: unknown = mesh;
		while (current) {
			const node = current as { name?: string; parent?: unknown };
			if (typeof node.name === "string" && node.name.includes(prefix)) return true;
			current = node.parent;
		}
		return false;
	};

	const getPuppetMeshMetadata = (mesh: { metadata?: unknown }) =>
		(mesh.metadata && typeof mesh.metadata === "object" ? mesh.metadata : {}) as PuppetMeshMetadata;
	const isPuppetArtPlaneMesh = (mesh: { metadata?: unknown }) =>
		getPuppetMeshMetadata(mesh).bumblebeePuppetPart === "art-plane";
	const isPuppetArtFinMesh = (mesh: { metadata?: unknown }) =>
		getPuppetMeshMetadata(mesh).bumblebeePuppetPart === "art-fin";
	const isArtMesh = (mesh: { name?: string; parent?: unknown; metadata?: unknown }) =>
		isPuppetArtPlaneMesh(mesh) ||
		isPuppetArtFinMesh(mesh) ||
		hasNamedAncestor(mesh, "puppet_art") ||
		hasNamedAncestor(mesh, "puppet_fin");
	const isCircleDimMesh = (name: string) => name.includes("puppet_circle_dim_");
	const isStickDimMesh = (name: string) => name.startsWith("stick_dim_");
	const isStickMesh = (meshName: string, materialName?: string) =>
		!isStickDimMesh(meshName) &&
		(meshName.startsWith("stick") ||
			meshName.includes("stick_") ||
			materialName?.startsWith("stickMat_") ||
			materialName?.startsWith("stick_edge_"));
	const resolveDepthEdgeArtPlanes = (meshes: AbstractMesh[]) => {
		const edgeMeshes = new Set<AbstractMesh>();
		if (!meshes.length) return edgeMeshes;
		const zValues = meshes.map((mesh) => mesh.position.z).filter(Number.isFinite);
		if (!zValues.length) return edgeMeshes;
		const minZ = Math.min(...zValues);
		const maxZ = Math.max(...zValues);
		const epsilon = 1e-7;
		for (const mesh of meshes) {
			if (
				Math.abs(mesh.position.z - minZ) <= epsilon ||
				Math.abs(mesh.position.z - maxZ) <= epsilon
			) {
				edgeMeshes.add(mesh);
			}
		}
		return edgeMeshes;
	};
	const resolveGhostEdgeArtPlanes = (meshes: AbstractMesh[]) => {
		const metadataArtPlanes = meshes.filter(isPuppetArtPlaneMesh);
		if (metadataArtPlanes.length) {
			const explicitEdges = metadataArtPlanes.filter((mesh) => {
				const edge = getPuppetMeshMetadata(mesh).bumblebeePuppetArtPlaneEdge;
				return edge === "front" || edge === "back";
			});
			return explicitEdges.length
				? new Set<AbstractMesh>(explicitEdges)
				: resolveDepthEdgeArtPlanes(metadataArtPlanes);
		}
		return resolveDepthEdgeArtPlanes(
			meshes.filter(
				(mesh) =>
					isArtMesh(mesh) && !isCircleDimMesh(mesh.name) && !hasNamedAncestor(mesh, "puppet_fin")
			)
		);
	};
	const setMaterialColorDim = (material: unknown, progress: number) => {
		const mat = material as
			| {
					metadata?: Record<string, unknown>;
					unfreeze?: () => void;
					albedoColor?: Color3;
					diffuseColor?: Color3;
			  }
			| null
			| undefined;
		if (!mat) return;
		mat.unfreeze?.();
		const metadata = (mat.metadata ??= {});
		const baseColors = (metadata.__puppetDimBaseColors ??= {}) as Record<string, Color3>;
		const multiplier = 1 - (1 - ART_DIM_COLOR_MULTIPLIER) * progress;
		for (const key of ["albedoColor", "diffuseColor"] as const) {
			const current = mat[key];
			if (!current) continue;
			baseColors[key] ??= current.clone();
			mat[key] = baseColors[key].scale(multiplier);
		}
	};

	const applyDimmedProgress = (progress: number) => {
		const normalizedProgress = Math.max(0, Math.min(1, progress));
		if (normalizedProgress > 0.001) {
			ensureStickDimOverlay();
		}
		for (const mesh of node.getChildMeshes(false)) {
			const circleDimMesh = isCircleDimMesh(mesh.name);
			const artMesh = !circleDimMesh && isArtMesh(mesh);
			const stickDimMesh = isStickDimMesh(mesh.name);
			const stickMesh = isStickMesh(mesh.name, mesh.material?.name);
			if (circleDimMesh) {
				mesh.renderOverlay = false;
				mesh.overlayAlpha = 0;
				mesh.visibility = puppetOpacity;
				if (mesh.material && "alpha" in mesh.material) {
					mesh.material.alpha = CIRCLE_DIM_OVERLAY_ALPHA * normalizedProgress;
				}
			} else if (artMesh) {
				mesh.renderOverlay = false;
				mesh.overlayAlpha = 0;
				mesh.visibility = puppetOpacity;
				setMaterialColorDim(mesh.material, normalizedProgress);
			} else if (stickDimMesh) {
				mesh.renderOverlay = false;
				mesh.overlayAlpha = 0;
				mesh.visibility = puppetOpacity;
				if (mesh.material && "alpha" in mesh.material) {
					mesh.material.alpha = STICK_DIM_ALPHA * normalizedProgress;
				}
			} else if (stickMesh) {
				mesh.renderOverlay = normalizedProgress > 0.001;
				mesh.overlayColor = Color3.Black();
				mesh.overlayAlpha = STICK_DIM_ALPHA * normalizedProgress;
				mesh.visibility =
					puppetOpacity * (1 - (1 - STICK_DIM_VISIBILITY_MULTIPLIER) * normalizedProgress);
			}
		}
		dimmedProgress = normalizedProgress;
	};

	const cancelDimmedAnimation = () => {
		if (dimmedAnimationFrame === null) return;
		cancelAnimationFrame(dimmedAnimationFrame);
		dimmedAnimationFrame = null;
	};

	const applyDimmedMode = (enabled: boolean, options?: { animate?: boolean }) => {
		cancelDimmedAnimation();
		const targetProgress = enabled ? 1 : 0;
		if (!options?.animate) {
			applyDimmedProgress(targetProgress);
			return;
		}
		const startProgress = dimmedProgress;
		const startMs = playbackNow(scene);
		const step = () => {
			const eased = Math.min(1, Math.max(0, (playbackNow(scene) - startMs) / DIM_ANIMATION_MS));
			const progress = startProgress + (targetProgress - startProgress) * eased;
			applyDimmedProgress(progress);
			if (eased < 1) {
				dimmedAnimationFrame = requestAnimationFrame(step);
			} else {
				dimmedAnimationFrame = null;
			}
		};
		dimmedAnimationFrame = requestAnimationFrame(step);
	};

	const applyGhostMode = (enabled: boolean) => {
		const meshes = node.getChildMeshes(false);
		if (!meshes.length) return;
		const edgeArtPlanes = resolveGhostEdgeArtPlanes(meshes);
		// Ghost mode should read like a single puppet "blueprint" (stick + character unified),
		// not a highlighted selection with outlines.
		let overlayColor = Color3.FromHexString("#8fefff");
		try {
			if (typeof stickColor === "string" && stickColor.trim().length) {
				overlayColor = Color3.FromHexString(stickColor);
			}
		} catch {}
		for (const mesh of meshes) {
			if (isCircleDimMesh(mesh.name)) {
				mesh.visibility = 0;
				mesh.renderOverlay = false;
				mesh.overlayAlpha = 0;
				mesh.renderOutline = false;
				mesh.outlineWidth = 0;
				mesh.disableEdgesRendering();
				continue;
			}
			const isArtPlane = isArtMesh(mesh);
			const isEdgeArtPlane = edgeArtPlanes.has(mesh);
			if (enabled && isArtPlane && !isEdgeArtPlane) {
				mesh.visibility = 0;
				mesh.renderOverlay = false;
				mesh.renderOutline = false;
				mesh.outlineWidth = 0;
				mesh.disableEdgesRendering();
				continue;
			}
			if (!enabled && isArtPlane && !isEdgeArtPlane) {
				mesh.visibility = 1;
			}
			if (isArtPlane && isEdgeArtPlane) {
				const mat = mesh.material as GhostDepthMaterial | null;
				if (enabled && mat) {
					const meta = (mesh.metadata ??= {}) as GhostDepthMetadata;
					if (!meta.ghostDepth) {
						meta.ghostDepth = {
							forceDepthWrite: mat.forceDepthWrite ?? false,
							alphaCutOff: mat.alphaCutOff,
							transparencyMode: mat.transparencyMode,
							needDepthPrePass: mat.needDepthPrePass ?? false
						};
					}
					mat.forceDepthWrite = true;
					if ("transparencyMode" in mat) {
						mat.transparencyMode = PBRMaterial.PBRMATERIAL_ALPHATESTANDBLEND;
					}
					if ("needDepthPrePass" in mat) {
						mat.needDepthPrePass = true;
					}
				} else if (!enabled && mat) {
					const meta = mesh.metadata as GhostDepthMetadata | null;
					if (meta?.ghostDepth) {
						mat.forceDepthWrite = meta.ghostDepth.forceDepthWrite ?? false;
						if ("transparencyMode" in mat && meta.ghostDepth.transparencyMode !== undefined) {
							mat.transparencyMode = meta.ghostDepth.transparencyMode;
						}
						if ("needDepthPrePass" in mat) {
							mat.needDepthPrePass = meta.ghostDepth.needDepthPrePass ?? false;
						}
						meta.ghostDepth = null;
					}
				}
			}
			// Keep everything "blueprinted" via overlay, while muting original materials so
			// the character matches the stick. Also: no outlines/edges in ghost mode.
			mesh.visibility = puppetOpacity;
			mesh.renderOverlay = enabled;
			mesh.overlayColor = overlayColor;
			mesh.overlayAlpha = enabled ? puppetOpacity : 0;
			mesh.renderOutline = false;
			mesh.outlineWidth = 0;
			mesh.disableEdgesRendering();
		}
		if (!enabled && dimmedModeEnabled) {
			applyDimmedMode(true);
		}
	};

	if (ghostModeEnabled) {
		applyGhostMode(true);
	}

	const setRenderGroupOffset = (offset: number) => {
		const safeOffset = Number.isFinite(offset) ? Math.round(offset) : 0;
		for (const mesh of node.getChildMeshes(false)) {
			const metadata = (mesh.metadata ??= {});
			const storedBase = (metadata as Record<string, unknown>).bumblebeeBaseRenderingGroupId;
			const baseRenderingGroupId =
				typeof storedBase === "number" ? storedBase : (mesh.renderingGroupId ?? 0);
			(metadata as Record<string, unknown>).bumblebeeBaseRenderingGroupId = baseRenderingGroupId;
			mesh.renderingGroupId = Math.min(3, Math.max(0, baseRenderingGroupId + safeOffset));
		}
	};

	const getVisibleArtBounds = () => {
		const bounds = node.metadata?.visibleArtBounds as
			| {
					left?: number;
					right?: number;
					top?: number;
					bottom?: number;
					width?: number;
					height?: number;
			  }
			| undefined;
		if (
			typeof bounds?.left !== "number" ||
			typeof bounds.right !== "number" ||
			typeof bounds.top !== "number" ||
			typeof bounds.bottom !== "number"
		) {
			return null;
		}
		return {
			left: bounds.left,
			right: bounds.right,
			top: bounds.top,
			bottom: bounds.bottom,
			width: bounds.width ?? bounds.right - bounds.left,
			height: bounds.height ?? bounds.top - bounds.bottom
		};
	};

	const getFallbackLocalBounds = () => {
		const dims = node.metadata?.localDims as { w?: number; h?: number } | undefined;
		if (!dims?.w || !dims?.h) {
			return { left: -1, right: 1, top: 4, bottom: 2 };
		}
		return {
			left: -dims.w / 2,
			right: dims.w / 2,
			top: dims.h,
			bottom: 0
		};
	};

	const getConfettiEmitterPose = () => {
		node.computeWorldMatrix(true);
		const origin = node.getAbsolutePosition().clone();
		const direction = Vector3.TransformNormal(new Vector3(0, 1, 0), node.getWorldMatrix());
		direction.z = 0;
		if (direction.lengthSquared() < 0.0001) {
			direction.copyFromFloats(0, 1, 0);
		}
		direction.normalize();
		return { origin, direction };
	};

	const projectLocalBoundsToScreen = (
		target: TransformNode,
		bounds: { left: number; right: number; top: number; bottom: number }
	) => {
		const ownScene = target.getScene();
		const camera = ownScene.activeCamera;
		const engine = ownScene.getEngine();
		const canvas = engine.getRenderingCanvas();
		if (!camera || !canvas) return null;
		const { cssWidth, cssHeight } = resolveCanvasViewportSize(canvas, engine);
		if (!cssWidth || !cssHeight) return null;
		const canvasRect = canvas.getBoundingClientRect();
		const viewport = new Viewport(0, 0, cssWidth, cssHeight);
		const transform = ownScene.getTransformMatrix();
		const world = target.computeWorldMatrix(true);
		const points = [
			new Vector3(bounds.left, bounds.top, 0),
			new Vector3(bounds.right, bounds.top, 0),
			new Vector3(bounds.right, bounds.bottom, 0),
			new Vector3(bounds.left, bounds.bottom, 0)
		].map((local) =>
			Vector3.Project(
				Vector3.TransformCoordinates(local, world),
				Matrix.Identity(),
				transform,
				viewport
			)
		);
		const xs = points.map((point) => point.x + canvasRect.left);
		const ys = points.map((point) => point.y + canvasRect.top);
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

	const projectLocalPointToScreen = (target: TransformNode, point: Vector3) => {
		const ownScene = target.getScene();
		const camera = ownScene.activeCamera;
		const engine = ownScene.getEngine();
		const canvas = engine.getRenderingCanvas();
		if (!camera || !canvas) return null;
		const { cssWidth, cssHeight } = resolveCanvasViewportSize(canvas, engine);
		if (!cssWidth || !cssHeight) return null;
		const canvasRect = canvas.getBoundingClientRect();
		const viewport = new Viewport(0, 0, cssWidth, cssHeight);
		const projected = Vector3.Project(
			Vector3.TransformCoordinates(point, target.computeWorldMatrix(true)),
			Matrix.Identity(),
			ownScene.getTransformMatrix(),
			viewport
		);
		if (![projected.x, projected.y].every(Number.isFinite)) return null;
		return {
			x: projected.x + canvasRect.left,
			y: projected.y + canvasRect.top,
			visible: node.isEnabled()
		};
	};

	const getNameplateRotationDeg = (target: TransformNode) => {
		const bottom = projectLocalPointToScreen(target, new Vector3(0, PUPPET_STICK_BOTTOM_Y, 0));
		const top = projectLocalPointToScreen(target, new Vector3(0, PUPPET_STICK_TOP_Y, 0));
		if (!bottom || !top) return 0;
		const dx = top.x - bottom.x;
		const dy = top.y - bottom.y;
		if (dx * dx + dy * dy < 0.0001) return 0;
		return (Math.atan2(dy, dx) * 180) / Math.PI + 90;
	};

	const getNameplateScale = (target: TransformNode) => {
		const bottom = projectLocalPointToScreen(target, new Vector3(0, PUPPET_STICK_BOTTOM_Y, 0));
		const top = projectLocalPointToScreen(target, new Vector3(0, PUPPET_STICK_TOP_Y, 0));
		if (!bottom || !top) return 1;
		const dx = top.x - bottom.x;
		const dy = top.y - bottom.y;
		const stickLengthPx = Math.sqrt(dx * dx + dy * dy);
		if (!Number.isFinite(stickLengthPx) || stickLengthPx <= 0) return 1;
		const scale = (stickLengthPx / NAMEPLATE_REFERENCE_STICK_PX) * NAMEPLATE_SCALE_MULTIPLIER;
		return Math.max(NAMEPLATE_MIN_SCALE, Math.min(NAMEPLATE_MAX_SCALE, scale));
	};

	return {
		node,
		contentNode,
		puppetId,
		setGhostMode(enabled: boolean) {
			ghostModeEnabled = enabled;
			cancelDimmedAnimation();
			applyGhostMode(enabled);
		},
		setDimmed(enabled: boolean) {
			dimmedModeEnabled = enabled;
			if (!ghostModeEnabled) {
				applyDimmedMode(enabled, { animate: true });
			}
		},
		applyOrganicTilt,
		cancelOrganicTiltTransition,
		setRenderGroupOffset,
		setOpacity(value: number) {
			const opacity = Math.max(0, Math.min(value, 1));
			puppetOpacity = opacity;
			if (ghostModeEnabled) {
				applyGhostMode(true);
			} else {
				applyDimmedProgress(dimmedProgress);
			}
		},
		setPuppetPosition(position: PuppetPosition, opts?: { animate?: boolean; durationMs?: number }) {
			positionManager.setPuppetPosition(position);
			const target = computeTargetFacingLeft();
			const wantsAnimation = !!opts?.animate && node.isEnabled();
			if (wantsAnimation) {
				return;
			}
			setFacingImmediate(target);
		},
		setTalkingSpeed(speed: number) {
			currentTalkingSpeed = Math.max(0.2, Math.min(speed, 3));
		},

		setTalkingIntensity(intensity: number) {
			currentTalkingIntensity = Math.max(0, Math.min(intensity, 1.4));
		},

		// Smoothly animate to the current spawn target using Babylon animation
		async moveToSpawn(options?: {
			durationMs?: number;
			targetScale?: number;
			preserveFacing?: boolean;
		}) {
			const seq = ++moveSeq;
			const isCurrent = () => seq === moveSeq;
			moveInFlight += 1;

			// Cancel the in-flight move so we "pause" mid-animation and begin a new move from
			// the current transform (matching Bumblebee moveTo behavior).
			try {
				activeMoveAbort?.abort();
			} catch {}
			const abort = new AbortController();
			activeMoveAbort = abort;

			const durationMs = options?.durationMs ?? 900;
			try {
				// Freeze existing animations affecting this node so start values are stable.
				scene.stopAnimation(node);
			} catch {}

			const { group, frames, flipFrame } = createMoveToSpawnAnimation(node, positionManager, {
				durationMs,
				targetScale: options?.targetScale
			});

			const targetFacingLeft = computeTargetFacingLeft();
			let flipTriggered = false;

			try {
				group.normalize(0, frames);
			} catch {}

			if (
				!options?.preserveFacing &&
				targetFacingLeft !== facingLeft &&
				typeof flipFrame === "number" &&
				frames > 0
			) {
				const posAnim = group.targetedAnimations.find(
					(entry) => entry.animation.targetProperty === "position"
				)?.animation;
				if (posAnim) {
					const flipDurationMs = 220;
					const flipFrames = Math.max(4, Math.round(frames * (flipDurationMs / durationMs)));
					const flipStartFrame = Math.max(
						0,
						Math.min(frames, Math.round(flipFrame - flipFrames / 2))
					);
					posAnim.addEvent(
						new AnimationEvent(
							flipStartFrame,
							() => {
								if (!isCurrent()) return;
								playFacingFlip(targetFacingLeft);
								flipTriggered = true;
							},
							true
						)
					);
				}
			}

			// We animate screen scale by animating position (distance) only.
			// Commit target scale at end so future calcs use it; mesh scaling stays at 1.
			const targetScale = options?.targetScale;
			if (typeof targetScale === "number") {
				group.onAnimationGroupEndObservable.add(() => {
					if (isCurrent()) {
						positionManager.setScalePercentage(targetScale);
					}
				});
			}

			try {
				await playAnimation({
					animationGroup: group,
					scene,
					blendMode: "none",
					resetToInitialOnStop: false,
					signal: abort.signal
				});
			} catch (error) {
				// Interrupted by a newer move; treat as a normal cancel.
				if (abort.signal.aborted || !isCurrent()) return;
				console.warn("Puppet moveToSpawn failed:", error);
				return;
			} finally {
				try {
					group.dispose();
				} catch {}
				if (activeMoveAbort === abort) {
					activeMoveAbort = null;
				}
				moveInFlight = Math.max(0, moveInFlight - 1);
			}

			if (!isCurrent()) return;
			if (typeof targetScale === "number") {
				positionManager.setScalePercentage(targetScale);
				node.position.copyFrom(positionManager.getWorldPosition());
			}
			if (!flipTriggered && !options?.preserveFacing) {
				setFacingImmediate(targetFacingLeft);
			}
		},

		async show(opts?: { disableTilt?: boolean; preserveFacing?: boolean }) {
			// Ensure we have some sane default if nothing set yet
			if (!puppetPosition) {
				positionManager.setViewportTarget({ xPercent: 0.8 });
			}

			// Pre-orient to face correctly before bringing on-screen unless a caller
			// has deliberately set an explicit facing override for this show cycle.
			if (!opts?.preserveFacing) {
				setFacingImmediate(computeTargetFacingLeft());
			}
			resetRotationToFacing();

			// Start below screen for bottom edge animation (scale-aware)
			const vt = positionManager.getViewportTarget();
			const h = vt.heightPercent;
			// Start fully off-screen (match show animation logic)
			const startY = -(h * 1.1);
			const startPos = positionManager.getWorldPosition({ yPercent: startY });
			node.position.copyFrom(startPos);

			// Enable puppet now that it's positioned properly
			// Use the current state in positionManager; don't override with stale
			const animationGroup = createShowAnimation(node, positionManager, undefined, {
				disableRoll: !!opts?.disableTilt
			});
			node.setEnabled(true);
			scene.render();
			try {
				scene.stopAnimation(node);
			} catch {}
			try {
				await playAnimation({
					animationGroup,
					scene,
					speed: 1.0,
					blendMode: "none",
					resetToInitialOnStop: false
				});
			} finally {
				animationGroup.dispose();
			}
		},

		async hide() {
			try {
				activeMoveAbort?.abort();
			} catch {}
			activeMoveAbort = null;
			try {
				scene.stopAnimation(node);
			} catch {}
			const animationGroup = createHideAnimation(node, positionManager);
			try {
				await playAnimation({
					animationGroup,
					scene,
					speed: 1.0,
					blendMode: "none",
					resetToInitialOnStop: false,
					strictEnd: true
				});
			} finally {
				animationGroup.dispose();
			}
		},

		alignFacingWithPosition,
		setFacingLeft(value: boolean) {
			setFacingImmediate(value);
		},
		setOcclusionRatio(
			value: number,
			opts?: {
				animate?: boolean;
				durationMs?: number;
				deferMove?: boolean;
				preserveFacing?: boolean;
			}
		) {
			positionManager.setOcclusionRatio(value);
			if (opts?.deferMove) return;
			if (opts?.animate && node.isEnabled()) {
				return this.moveToSpawn({
					durationMs: opts.durationMs,
					targetScale: positionManager.getCurrentScale(),
					preserveFacing: opts.preserveFacing
				});
			}
			const targetWorld = positionManager.getWorldPosition();
			node.position.copyFrom(targetWorld);
		},
		getOcclusionRatio: () => positionManager.getOcclusionRatio(),
		captureSpeechOcclusionBaseline() {
			return {
				occlusionRatio: positionManager.getOcclusionRatio(),
				position: node.position.clone()
			};
		},
		applySpeechOcclusionRatio(
			value: number,
			baseline?: { occlusionRatio: number; position: Vector3 }
		) {
			const resolvedBaseline = baseline ?? {
				occlusionRatio: positionManager.getOcclusionRatio(),
				position: node.position.clone()
			};
			const baseWorld = positionManager.getWorldPositionForOcclusionRatio(
				resolvedBaseline.occlusionRatio
			);
			const targetWorld = positionManager.getWorldPositionForOcclusionRatio(value);
			node.position.copyFrom(resolvedBaseline.position.add(targetWorld.subtract(baseWorld)));
			positionManager.setOcclusionRatio(value);
		},
		getScalePercentage: () => positionManager.getCurrentScale(),
		isFacingLeft: () => facingLeft,
		getConfettiEmitterPose,
		getVisibleArtLocalBounds() {
			return getVisibleArtBounds();
		},
		getChatDecorationScreenRect(opts?: { stable?: boolean }) {
			const bounds = getVisibleArtBounds() ?? getFallbackLocalBounds();
			return projectLocalBoundsToScreen(opts?.stable === false ? contentNode : node, bounds);
		},
		getNameplateScreenAnchor(opts?: { stable?: boolean }) {
			const bounds = getVisibleArtBounds() ?? getFallbackLocalBounds();
			const local = getPuppetNameplateLocalAnchor(bounds);
			const target = opts?.stable === false ? contentNode : node;
			const anchor = projectLocalPointToScreen(target, new Vector3(local.x, local.y, 0));
			if (!anchor) return null;
			return {
				...anchor,
				scale: getNameplateScale(target),
				rotationDeg: getNameplateRotationDeg(target)
			};
		},
		async warmup() {
			const meshes = node.getChildMeshes(false);
			const tasks = meshes
				.map((mesh) => {
					const material = mesh.material as
						| {
								forceCompilationAsync?: (mesh: unknown) => Promise<unknown>;
						  }
						| null
						| undefined;
					return material?.forceCompilationAsync?.(mesh);
				})
				.filter((task): task is Promise<unknown> => !!task);
			if (tasks.length) {
				await Promise.allSettled(tasks);
			}
			scene.render();
		},

		setScalePercentage(value: number) {
			// Update target scale; snap position to new world immediately
			positionManager.setScalePercentage(value);
			const world = positionManager.getWorldPosition();
			node.position.copyFrom(world);
		},

		async talk() {
			if (talkingObserver) return;
			talkingReturnAnimation?.stop();
			talkingReturnAnimation = null;
			talkingRestRotationZ = talkTarget.rotation.z;
			if (talkTarget.rotationQuaternion) talkTarget.rotationQuaternion = null;
			let lastMs = playbackNow(scene);
			talkingObserver = scene.onBeforeRenderObservable.add(() => {
				if (playbackPaused(scene)) return;
				const nowMs = playbackNow(scene);
				const dtSeconds = Math.max(1 / 120, Math.min(0.12, (nowMs - lastMs) / 1000));
				lastMs = nowMs;
				const baseHz = PUPPET_TALK_HZ;
				talkingPhase += dtSeconds * baseHz * Math.PI * 2 * currentTalkingSpeed;
				const rest = talkingRestRotationZ ?? 0;
				talkTarget.rotation.z = rest + puppetSpeechRotation(talkingPhase, currentTalkingIntensity);
			});
		},

		async shutup() {
			if (!talkingObserver) return;
			scene.onBeforeRenderObservable.remove(talkingObserver);
			talkingObserver = null;
			if (talkingRestRotationZ !== null) {
				const rest = talkingRestRotationZ;
				const start = talkTarget.rotation.z;
				const returnAnim = new Animation(
					"puppetTalkingReturn",
					"rotation.z",
					60,
					Animation.ANIMATIONTYPE_FLOAT,
					Animation.ANIMATIONLOOPMODE_CONSTANT
				);
				returnAnim.setKeys([
					{ frame: 0, value: start },
					{ frame: 8, value: rest }
				]);
				talkingReturnAnimation?.stop();
				talkingReturnAnimation = scene.beginDirectAnimation(
					talkTarget,
					[returnAnim],
					0,
					8,
					false,
					1
				);
				talkingReturnAnimation.onAnimationEndObservable.addOnce(() => {
					talkTarget.rotation.z = rest;
					if (talkingReturnAnimation) talkingReturnAnimation = null;
				});
				talkingRestRotationZ = null;
			}
		},

		async dispose() {
			try {
				activeMoveAbort?.abort();
			} catch {}
			activeMoveAbort = null;
			try {
				scene.stopAnimation(node);
			} catch {}
			cancelDimmedAnimation();
			if (talkingObserver) {
				scene.onBeforeRenderObservable.remove(talkingObserver);
				talkingObserver = null;
			}
			talkingReturnAnimation?.stop();
			organicTiltSeq += 1;
			organicTiltAnimation?.stop();
			positionManager.dispose();
			const materials = new Set(
				node
					.getChildMeshes(false)
					.map((mesh) => mesh.material)
					.filter((material): material is NonNullable<typeof material> =>
						Boolean(
							(material as { metadata?: Record<string, unknown> } | null)?.metadata?.[
								"__puppetClonedMaterial"
							]
						)
					)
			);
			node.dispose();
			for (const material of materials) {
				(material as { dispose?: (...args: unknown[]) => void }).dispose?.(false, false, true);
			}
		}
	};
};

type PuppetController = Awaited<ReturnType<typeof createPuppet>>;

export type { PuppetController };

export { createPuppet };
