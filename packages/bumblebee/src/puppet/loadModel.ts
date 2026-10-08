import type { AbstractMesh } from "@babylonjs/core/Meshes/abstractMesh";
import { Constants } from "@babylonjs/core/Engines/constants";
import { Matrix, Vector3 } from "@babylonjs/core/Maths/math.vector";
import { Material } from "@babylonjs/core/Materials/material";
import { Mesh } from "@babylonjs/core/Meshes/mesh";
import { MeshBuilder } from "@babylonjs/core/Meshes/meshBuilder";
import { PBRMaterial } from "@babylonjs/core/Materials/PBR/pbrMaterial";
import { Scene } from "@babylonjs/core/scene";
import { StandardMaterial } from "@babylonjs/core/Materials/standardMaterial";
import { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import { createWoodGrainTexture } from "./createWoodGrainTexture";
import { createImageTexture } from "./createImageTexture";
import z from "zod";
import type {
	PuppetRenderableMetadata,
	PuppetRenderColor,
	PuppetRenderMetadata,
	PuppetSilhouetteMeasurements
} from "../types";
import {
	buildPuppetAlphaBounds,
	buildPuppetRenderMetadata,
	metadataCacheKey,
	normalizePuppetRenderRibbonPath,
	renderMetadataContentKey
} from "./renderMetadata";
import {
	buildCenteredArtPlaneMeasurements,
	PUPPET_ART_PLANE_SIZE,
	PUPPET_STICK_BOTTOM_Y,
	PUPPET_STICK_RADIUS,
	PUPPET_STICK_TOP_Y
} from "./geometry";
import { DEFAULT_PUPPET_TEXTURE_RESOLUTION } from "../utils/babylonPerformance";

type ReadbackEntry = { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D };
type TextureReadbackSource = object & {
	getContext?: () => CanvasRenderingContext2D | null;
};
const readbackCache = new WeakMap<object, ReadbackEntry>();

const getTextureReadContext = (tex: unknown): CanvasRenderingContext2D | null => {
	if (!tex || typeof tex !== "object") return null;
	const texture = tex as TextureReadbackSource;

	const baseCtx = texture.getContext?.() ?? null;
	const srcCanvas = baseCtx?.canvas as HTMLCanvasElement | undefined;
	if (!baseCtx || !srcCanvas) return baseCtx;

	let entry = readbackCache.get(texture);
	if (!entry) {
		const readCanvas = document.createElement("canvas");
		readCanvas.width = srcCanvas.width;
		readCanvas.height = srcCanvas.height;
		const readCtx = readCanvas.getContext("2d", { willReadFrequently: true });
		if (!readCtx) {
			return baseCtx;
		}
		entry = { canvas: readCanvas, ctx: readCtx };
		readbackCache.set(texture, entry);
	} else if (entry.canvas.width !== srcCanvas.width || entry.canvas.height !== srcCanvas.height) {
		entry.canvas.width = srcCanvas.width;
		entry.canvas.height = srcCanvas.height;
	}

	entry.ctx.clearRect(0, 0, entry.canvas.width, entry.canvas.height);
	entry.ctx.drawImage(srcCanvas, 0, 0, entry.canvas.width, entry.canvas.height);
	return entry.ctx;
};
import { Color3 } from "@babylonjs/core/Maths/math.color";

// Extend Scene to hold cached meshes
interface SceneWithCache extends Scene {
	metadata: Record<string, unknown>;
}

type LoadedPuppetModel = {
	node: TransformNode;
	contentNode: TransformNode;
	animationGroups: unknown[];
	ensureStickDimOverlay: () => void;
};

type PuppetTemplate = {
	node: TransformNode;
	metadata: Record<string, unknown>;
	liveClones: number;
	evicted: boolean;
	disposed: boolean;
};

const ART_HALF_THICKNESS = 0.03;
const STICK_HALF_THICKNESS = ART_HALF_THICKNESS * 0.5;
const PUPPET_PLANE_GAP = 0.00125;
const FULL_ART_PLANE_COUNT = 32;
const FULL_STICK_PLANE_COUNT = 12;
const HALF_ART_PLANE_COUNT = FULL_ART_PLANE_COUNT / 2;
const HALF_STICK_PLANE_COUNT = FULL_STICK_PLANE_COUNT / 2;
const FLAT_ART_PLANE_COUNT = 2;
const FLAT_STICK_PLANE_COUNT = 1;
const CIRCLE_IMAGE_MASK_RADIUS = 0.905;
const MAX_PUPPET_TEMPLATE_CACHE_ENTRIES = 48;
const MAX_COMPILED_SILHOUETTE_CACHE_ENTRIES = 96;
const PUPPET_TEMPLATE_VERSION = "solid-ghost-dim-overlays-multi-art-fin-depth-planes-v20";
const PUPPET_RUNTIME_RENDER_METADATA_VERSION = "validated-ribbon-fill-edge-v5";
const STICK_COLOR_PATTERN = /^#[0-9a-fA-F]{6}$/;

const hashString = (value: string) => {
	let hash = 0x811c9dc5;
	for (let i = 0; i < value.length; i++) {
		hash ^= value.charCodeAt(i);
		hash = Math.imul(hash, 0x01000193);
	}
	return (hash >>> 0).toString(16);
};

const deepCloneMetadata = <T>(value: T): T => {
	if (value == null || typeof value !== "object") return value;
	try {
		return structuredClone(value);
	} catch {
		return JSON.parse(JSON.stringify(value)) as T;
	}
};

const getTemplateCache = (scene: SceneWithCache) => {
	scene.metadata ||= {};
	let cache = scene.metadata.__puppetModelTemplates as Map<string, PuppetTemplate> | undefined;
	if (!cache) {
		cache = new Map<string, PuppetTemplate>();
		scene.metadata.__puppetModelTemplates = cache;
		scene.onDisposeObservable.addOnce(() => {
			if (!cache) return;
			for (const template of cache.values()) {
				evictTemplate(template);
			}
			cache.clear();
		});
	}
	return cache;
};

const getCompiledSilhouetteCache = (scene: SceneWithCache) => {
	scene.metadata ||= {};
	let cache = scene.metadata.__puppetCompiledSilhouettes as
		Map<string, PuppetRenderMetadata | null> | undefined;
	if (!cache) {
		cache = new Map<string, PuppetRenderMetadata | null>();
		scene.metadata.__puppetCompiledSilhouettes = cache;
		scene.onDisposeObservable.addOnce(() => {
			cache?.clear();
		});
	}
	return cache;
};

const getCachedCompiledSilhouette = (scene: SceneWithCache, key: string) => {
	const cache = getCompiledSilhouetteCache(scene);
	if (!cache.has(key)) return undefined;
	const value = cache.get(key) ?? null;
	cache.delete(key);
	cache.set(key, value);
	return value;
};

const setCachedCompiledSilhouette = (
	scene: SceneWithCache,
	key: string,
	value: PuppetRenderMetadata | null
) => {
	const cache = getCompiledSilhouetteCache(scene);
	cache.set(key, value);
	while (cache.size > MAX_COMPILED_SILHOUETTE_CACHE_ENTRIES) {
		const oldest = cache.keys().next().value as string | undefined;
		if (!oldest) return;
		cache.delete(oldest);
	}
};

const disposeTemplate = (template: PuppetTemplate) => {
	if (template.disposed) return;
	template.disposed = true;
	template.node.dispose(false, true);
};

const evictTemplate = (template: PuppetTemplate) => {
	template.evicted = true;
	if (template.liveClones === 0) {
		disposeTemplate(template);
	}
};

const trimTemplateCache = (cache: Map<string, PuppetTemplate>) => {
	while (cache.size > MAX_PUPPET_TEMPLATE_CACHE_ENTRIES) {
		const oldest = cache.entries().next().value as [string, PuppetTemplate] | undefined;
		if (!oldest) return;
		const [key, template] = oldest;
		cache.delete(key);
		evictTemplate(template);
	}
};

const setTemplateCacheEntry = (
	cache: Map<string, PuppetTemplate>,
	key: string,
	template: PuppetTemplate
) => {
	cache.set(key, template);
	trimTemplateCache(cache);
};

const getCachedTemplate = (cache: Map<string, PuppetTemplate>, key: string) => {
	const template = cache.get(key);
	if (!template) return undefined;
	cache.delete(key);
	cache.set(key, template);
	return template;
};

const isPuppetRenderMetadata = (
	metadata?: PuppetRenderableMetadata | null
): metadata is PuppetRenderMetadata =>
	Boolean(
		metadata &&
		typeof metadata === "object" &&
		"version" in metadata &&
		metadata.version === 2 &&
		"silhouette" in metadata &&
		Array.isArray(metadata.silhouette.ribbons)
	);

type RenderMode = "full" | "half" | "flat";
type PuppetArtPlaneMetadata = {
	bumblebeePuppetPart: "art-plane";
	bumblebeePuppetArtPlaneIndex: number;
	bumblebeePuppetArtPlaneCount: number;
	bumblebeePuppetArtPlaneEdge?: "front" | "back";
};
type PuppetFinMetadata = {
	bumblebeePuppetPart: "art-fin";
};

const shouldCreatePuppetArtFin = (renderMode: RenderMode) => renderMode !== "flat";

const resolvePuppetDepthLayout = (renderMode: RenderMode) => {
	const artPlaneCount =
		renderMode === "flat"
			? FLAT_ART_PLANE_COUNT
			: renderMode === "half"
				? HALF_ART_PLANE_COUNT
				: FULL_ART_PLANE_COUNT;
	const stickPlaneCount =
		renderMode === "flat"
			? FLAT_STICK_PLANE_COUNT
			: renderMode === "half"
				? HALF_STICK_PLANE_COUNT
				: FULL_STICK_PLANE_COUNT;
	const totalPlaneCount = artPlaneCount + stickPlaneCount;
	const slots = Array.from({ length: totalPlaneCount }, (_, index) => ({
		index,
		z: (index - (totalPlaneCount - 1) / 2) * PUPPET_PLANE_GAP
	}));

	const rings = new Map<string, typeof slots>();
	for (const slot of slots) {
		const key = Math.abs(slot.z).toFixed(8);
		const ring = rings.get(key);
		if (ring) {
			ring.push(slot);
		} else {
			rings.set(key, [slot]);
		}
	}

	const orderedRings = Array.from(rings.values()).sort(
		(a, b) => Math.abs(a[0].z) - Math.abs(b[0].z)
	);
	const stickSlots = new Set<number>();
	for (let ringIndex = 0; stickSlots.size < stickPlaneCount; ringIndex += 2) {
		const ring = orderedRings[ringIndex];
		if (!ring) break;
		for (const slot of ring.sort((a, b) => a.z - b.z)) {
			if (stickSlots.size >= stickPlaneCount) break;
			stickSlots.add(slot.index);
		}
	}

	const artZ: number[] = [];
	const stickZ: number[] = [];
	for (const slot of slots) {
		if (stickSlots.has(slot.index)) {
			stickZ.push(slot.z);
		} else {
			artZ.push(slot.z);
		}
	}

	return {
		artZ: artZ.sort((a, b) => a - b),
		stickZ: stickZ.sort((a, b) => a - b)
	};
};

const color3FromRenderColor = (color?: PuppetRenderColor | null) =>
	color
		? new Color3(
				Math.max(0, Math.min(1, color.r)),
				Math.max(0, Math.min(1, color.g)),
				Math.max(0, Math.min(1, color.b))
			)
		: new Color3(0.08, 0.08, 0.08);

const createFinMaterial = (scene: Scene, name: string, color: Color3, texture?: unknown) => {
	const material = new StandardMaterial(name, scene);
	material.disableLighting = true;
	if (texture) {
		material.emissiveTexture = texture as StandardMaterial["emissiveTexture"];
	} else {
		material.diffuseColor = color;
		material.emissiveColor = color;
	}
	material.backFaceCulling = false;
	material.freeze();
	return material;
};

const createRibbonFin = (options: {
	scene: Scene;
	name: string;
	parent: TransformNode;
	path: Vector3[];
	frontZ: number;
	backZ: number;
	material: StandardMaterial;
	renderingGroupId: number;
}) => {
	const { scene, name, parent, path, frontZ, backZ, material, renderingGroupId } = options;
	if (
		path.length < 3 ||
		!Number.isFinite(frontZ) ||
		!Number.isFinite(backZ) ||
		path.some(
			(point) => !Number.isFinite(point.x) || !Number.isFinite(point.y) || !Number.isFinite(point.z)
		) ||
		Math.abs(backZ - frontZ) < 1e-6
	) {
		return null;
	}
	const frontPath = path.map((point) => new Vector3(point.x, point.y, frontZ));
	const backPath = path.map((point) => new Vector3(point.x, point.y, backZ));
	const fin = MeshBuilder.CreateRibbon(
		name,
		{
			pathArray: [frontPath, backPath],
			closePath: true,
			closeArray: false,
			sideOrientation: Mesh.DOUBLESIDE
		},
		scene
	);
	fin.parent = parent;
	fin.renderingGroupId = renderingGroupId;
	fin.material = material;
	fin.metadata = {
		...(fin.metadata ?? {}),
		bumblebeePuppetPart: "art-fin"
	} satisfies PuppetFinMetadata;
	return fin;
};

const createStickOutlinePath = () => {
	const halfWidth = PUPPET_STICK_RADIUS;
	const topY = PUPPET_STICK_TOP_Y;
	const bottomY = PUPPET_STICK_BOTTOM_Y;
	const arcSegments = 32;
	const edgeSegments = 32;
	const topArc: Vector3[] = [];
	for (let i = 0; i <= arcSegments; i++) {
		const t = i / arcSegments;
		const angle = Math.PI * t;
		topArc.push(new Vector3(halfWidth * Math.cos(angle), topY + halfWidth * Math.sin(angle), 0));
	}
	const leftEdge: Vector3[] = [];
	const straightLength = topY - halfWidth - (bottomY + halfWidth);
	for (let i = 1; i < edgeSegments; i++) {
		leftEdge.push(
			new Vector3(-halfWidth, topY - halfWidth - (i * straightLength) / edgeSegments, 0)
		);
	}
	const bottomArc: Vector3[] = [];
	for (let i = 0; i <= arcSegments; i++) {
		const t = i / arcSegments;
		const angle = Math.PI + Math.PI * t;
		bottomArc.push(
			new Vector3(halfWidth * Math.cos(angle), bottomY + halfWidth * Math.sin(angle), 0)
		);
	}
	const rightEdge: Vector3[] = [];
	for (let i = 1; i < edgeSegments; i++) {
		rightEdge.push(
			new Vector3(halfWidth, bottomY + halfWidth + (i * straightLength) / edgeSegments, 0)
		);
	}
	return [...topArc, ...leftEdge, ...bottomArc, ...rightEdge];
};

const puppetTemplateKey = (options: {
	puppetId: string;
	imageUrl?: string;
	imageMask?: string;
	stickColor: string;
	renderMode: RenderMode;
	textureResolution?: number;
	renderMetadata?: PuppetRenderableMetadata | null;
}) =>
	[
		PUPPET_TEMPLATE_VERSION,
		options.puppetId,
		options.imageUrl ?? "",
		options.imageMask ?? "none",
		options.stickColor,
		options.renderMode,
		options.textureResolution ?? "",
		renderMetadataContentKey(options.renderMetadata)
	]
		.map((part) => hashString(String(part)))
		.join("_");

const findContentNode = (node: TransformNode) =>
	node.getChildTransformNodes(false).find((child) => child.name.includes("puppet_content_")) as
		TransformNode | undefined;

const uniquifyCloneMaterials = (root: TransformNode, suffix: string) => {
	const clonedMaterials = new Map<unknown, unknown>();
	const copyTextureRefs = (source: unknown, target: unknown) => {
		const sourceMaterial = source as Record<string, unknown>;
		const targetMaterial = target as Record<string, unknown>;
		for (const key of [
			"albedoTexture",
			"emissiveTexture",
			"diffuseTexture",
			"opacityTexture",
			"bumpTexture"
		]) {
			if (key in sourceMaterial) {
				targetMaterial[key] = sourceMaterial[key];
			}
		}
	};
	for (const mesh of root.getChildMeshes(false) as AbstractMesh[]) {
		const material = mesh.material;
		if (!material) continue;
		if (clonedMaterials.has(material)) {
			mesh.material = clonedMaterials.get(material) as typeof material;
			continue;
		}
		const clone = material.clone?.(`${material.name}_${suffix}`) ?? material;
		if (clone !== material) {
			copyTextureRefs(material, clone);
			(clone as { metadata?: Record<string, unknown> }).metadata = {
				...((clone as { metadata?: Record<string, unknown> }).metadata ?? {}),
				__puppetClonedMaterial: true
			};
		}
		clonedMaterials.set(material, clone);
		mesh.material = clone;
	}
};

const cloneTemplate = (template: PuppetTemplate, scene: SceneWithCache): LoadedPuppetModel => {
	const suffix = `${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
	const node = template.node.clone(`puppet_${suffix}`, null, false) as TransformNode | null;
	if (!node) {
		throw new Error("Failed to clone puppet model template");
	}
	template.liveClones += 1;
	let releasedTemplate = false;
	const releaseTemplate = () => {
		if (releasedTemplate) return;
		releasedTemplate = true;
		template.liveClones = Math.max(0, template.liveClones - 1);
		if (template.evicted && template.liveClones === 0) {
			disposeTemplate(template);
		}
	};
	const originalDispose = node.dispose.bind(node);
	node.dispose = ((...args: Parameters<TransformNode["dispose"]>) => {
		try {
			return originalDispose(...args);
		} finally {
			releaseTemplate();
		}
	}) as TransformNode["dispose"];
	node.metadata = deepCloneMetadata(template.metadata);
	uniquifyCloneMaterials(node, suffix);
	node.setEnabled(false);
	const contentNode = findContentNode(node);
	if (!contentNode) {
		node.dispose();
		throw new Error("Failed to clone puppet content node from template");
	}
	let stickDimOverlayCreated = false;
	return {
		node,
		contentNode,
		animationGroups: [],
		ensureStickDimOverlay: () => {
			if (stickDimOverlayCreated) return;
			stickDimOverlayCreated = true;
			createStickDimOverlay(scene, suffix, contentNode);
		}
	};
};

const OptionsSchema = z.object({
	puppetId: z.string(),
	imageUrl: z.string().optional(),
	imageMask: z.enum(["none", "circle"]).optional(),
	scene: z.instanceof(Scene),
	stickColor: z.string().regex(STICK_COLOR_PATTERN).optional().default("#FFCC88"),
	renderMode: z.enum(["full", "half", "flat"]).optional().default("flat"),
	textureResolution: z.number().int().min(256).max(4096).optional(),
	renderMetadata: z.custom<PuppetRenderableMetadata | null>().optional()
});

type Options = z.input<typeof OptionsSchema>;

//------------------------------------------------------------------------------
// Create or get cached stick mesh with rounded ends
//------------------------------------------------------------------------------
async function ensureStickMesh(scene: SceneWithCache, stickColor: string): Promise<Mesh> {
	scene.metadata = scene.metadata || {};
	const cacheKey = `stickMesh_${stickColor}`;

	if (!scene.metadata[cacheKey]) {
		// Create stick body plane positioned so bottom is at origin
		const stickHeight = PUPPET_STICK_TOP_Y - PUPPET_STICK_BOTTOM_Y;
		const stickBody = MeshBuilder.CreatePlane(
			"stickBody",
			{ width: PUPPET_STICK_RADIUS * 2, height: stickHeight },
			scene
		);
		stickBody.position.y = PUPPET_STICK_BOTTOM_Y + stickHeight / 2;

		// Create top and bottom rounded caps using circles
		const topCap = MeshBuilder.CreateDisc(
			"stickTopCap",
			{ radius: PUPPET_STICK_RADIUS, tessellation: 16 },
			scene
		);
		topCap.position.y = PUPPET_STICK_TOP_Y;
		topCap.position.z = 0.001; // Slightly in front

		const bottomCap = MeshBuilder.CreateDisc(
			"stickBottomCap",
			{ radius: PUPPET_STICK_RADIUS, tessellation: 16 },
			scene
		);
		bottomCap.position.y = PUPPET_STICK_BOTTOM_Y;
		bottomCap.position.z = 0.001; // Slightly in front

		// Merge all stick parts into a single mesh using Mesh.MergeMeshes
		const stickMesh = Mesh.MergeMeshes(
			[stickBody, topCap, bottomCap],
			true,
			true,
			undefined,
			false,
			true
		);
		if (!stickMesh) {
			throw new Error("Failed to merge stick meshes");
		}
		stickMesh.name = "stickBase";

		// Create shared wood grain material
		const woodTexture = createWoodGrainTexture(
			`stickTexture_${stickColor}`,
			256,
			256,
			stickColor,
			scene
		);

		const mat = new StandardMaterial(`stickMat_${stickColor}`, scene);
		mat.disableLighting = true;
		mat.emissiveTexture = woodTexture;
		mat.backFaceCulling = false; // Make stick visible from both sides
		// Material is static; freeze for perf
		mat.freeze();

		// Apply material to merged mesh
		stickMesh.material = mat;

		// Center stick geometry on local Z so internal caps don't protrude beyond art extremes
		const bbox = stickMesh.getBoundingInfo().boundingBox;
		const minZ = bbox.minimumWorld.z;
		const maxZ = bbox.maximumWorld.z;
		const centerZ = (minZ + maxZ) / 2;
		if (Math.abs(centerZ) > 1e-6) {
			stickMesh.bakeTransformIntoVertices(Matrix.Translation(0, 0, -centerZ));
		}

		// No setPivotPoint needed - we'll position geometry relative to desired origin
		stickMesh.setEnabled(false); // Disabled base for cloning

		scene.metadata[cacheKey] = stickMesh;
	}

	return scene.metadata[cacheKey] as Mesh;
}

function createStickDimOverlay(scene: Scene, puppetId: string, parent: TransformNode) {
	const mat = new StandardMaterial(`stick_dim_mat_${puppetId}`, scene);
	mat.disableLighting = true;
	mat.diffuseColor = Color3.Black();
	mat.emissiveColor = Color3.Black();
	mat.alpha = 0;
	mat.backFaceCulling = false;
	mat.transparencyMode = Material.MATERIAL_ALPHABLEND;
	mat.disableDepthWrite = true;
	const overlayZ = STICK_HALF_THICKNESS + 0.006;

	const body = MeshBuilder.CreatePlane(
		`stick_dim_${puppetId}_body`,
		{ width: 0.22, height: 3, sideOrientation: Mesh.DOUBLESIDE },
		scene
	);
	body.parent = parent;
	body.position.y = 1.5;
	body.position.z = overlayZ;
	body.renderingGroupId = 1;
	body.material = mat;

	const topCap = MeshBuilder.CreateDisc(
		`stick_dim_${puppetId}_top`,
		{ radius: 0.11, tessellation: 32, sideOrientation: Mesh.DOUBLESIDE },
		scene
	);
	topCap.parent = parent;
	topCap.position.y = 3;
	topCap.position.z = overlayZ;
	topCap.renderingGroupId = 1;
	topCap.material = mat;

	const bottomCap = MeshBuilder.CreateDisc(
		`stick_dim_${puppetId}_bottom`,
		{ radius: 0.11, tessellation: 32, sideOrientation: Mesh.DOUBLESIDE },
		scene
	);
	bottomCap.parent = parent;
	bottomCap.position.y = 0;
	bottomCap.position.z = overlayZ;
	bottomCap.renderingGroupId = 1;
	bottomCap.material = mat;
}

//------------------------------------------------------------------------------
// Create puppet art plane(s) with proper occlusion
//------------------------------------------------------------------------------
async function createPuppetPlanes(
	puppetId: string,
	scene: Scene,
	mode: RenderMode,
	textureResolutionOverride?: number,
	imageUrlOverride?: string,
	imageMaskOverride?: "none" | "circle",
	imageRingColorOverride?: string | null,
	renderMetadata?: PuppetRenderableMetadata | null
): Promise<{
	node: TransformNode;
	visibleHeight?: number;
	visibleArtBounds?: {
		left: number;
		right: number;
		top: number;
		bottom: number;
		width: number;
		height: number;
	};
}> {
	const imageUrl =
		typeof imageUrlOverride === "string" && imageUrlOverride.trim().length
			? imageUrlOverride
			: `./puppets/images/${encodeURIComponent(puppetId)}.png`;
	const imageMask = imageMaskOverride ?? "none";
	const imageRingColor = typeof imageRingColorOverride === "string" ? imageRingColorOverride : null;

	// Create container for art planes
	const artNode = new TransformNode(`puppet_art_${puppetId}`, scene);

	// Create material with puppet texture for front plane
	const defaultResolution = DEFAULT_PUPPET_TEXTURE_RESOLUTION;
	const textureResolution = textureResolutionOverride ?? defaultResolution;
	const contourSampleResolution = Math.max(128, Math.min(textureResolution, 1024));
	const avatarTexture = await createImageTexture(
		`avatar_${puppetId}`,
		imageUrl,
		scene,
		textureResolution,
		{ mask: imageMask, ringColor: imageRingColor }
	);
	const createCircleDimOverlay = (z: number) => {
		if (imageMask !== "circle") return;
		const dimMat = new StandardMaterial(`puppet_circle_dim_mat_${puppetId}`, scene);
		dimMat.diffuseColor = Color3.Black();
		dimMat.emissiveColor = Color3.Black();
		dimMat.disableLighting = true;
		dimMat.alpha = 0;
		dimMat.transparencyMode = Material.MATERIAL_ALPHABLEND;
		dimMat.disableDepthWrite = true;
		dimMat.depthFunction = Constants.ALWAYS;
		dimMat.backFaceCulling = false;

		const dimDisc = MeshBuilder.CreateDisc(
			`puppet_circle_dim_${puppetId}`,
			{ radius: CIRCLE_IMAGE_MASK_RADIUS, tessellation: 96, sideOrientation: Mesh.DOUBLESIDE },
			scene
		);
		dimDisc.parent = artNode;
		dimDisc.position.z = z;
		dimDisc.renderingGroupId = 3;
		dimDisc.material = dimMat;
	};
	let artFinMetadata: PuppetRenderMetadata | null =
		shouldCreatePuppetArtFin(mode) && isPuppetRenderMetadata(renderMetadata)
			? renderMetadata
			: null;
	let measuredVisibleHeight: number | undefined;
	let measuredVisibleArtBounds:
		| { left: number; right: number; top: number; bottom: number; width: number; height: number }
		| undefined;
	try {
		const applySilhouetteMeasurements = (measurements: PuppetSilhouetteMeasurements) => {
			measuredVisibleHeight = measurements.visibleHeight;
			measuredVisibleArtBounds = measurements.visibleArtBounds;
		};

		if (isPuppetRenderMetadata(renderMetadata)) {
			applySilhouetteMeasurements(renderMetadata.silhouette);
		} else if (renderMetadata) {
			applySilhouetteMeasurements(renderMetadata);
		}

		const needsMeasuredBounds = measuredVisibleHeight === undefined;
		const needsRibbonMetadata = shouldCreatePuppetArtFin(mode) && !artFinMetadata;
		const ctx =
			needsMeasuredBounds || needsRibbonMetadata ? getTextureReadContext(avatarTexture) : null;
		const canvas = ctx?.canvas as HTMLCanvasElement | undefined;
		if (ctx && canvas) {
			const srcW = canvas.width;
			const srcH = canvas.height;
			const maxDim = contourSampleResolution;
			const img = ctx.getImageData(0, 0, srcW, srcH).data;
			if (needsRibbonMetadata) {
				const cacheKey = `puppetCompiledSilhouette_${PUPPET_RUNTIME_RENDER_METADATA_VERSION}_${puppetId}_${metadataCacheKey(
					imageUrl,
					`${imageMask}:${imageRingColor ?? ""}`,
					maxDim
				)}`;
				let compiled = getCachedCompiledSilhouette(scene as SceneWithCache, cacheKey);
				if (compiled === undefined) {
					compiled =
						buildPuppetRenderMetadata(
							{ width: srcW, height: srcH, data: img },
							{ sampleResolution: maxDim }
						) ?? null;
					setCachedCompiledSilhouette(scene as SceneWithCache, cacheKey, compiled);
				}
				if (compiled) {
					artFinMetadata = compiled;
					applySilhouetteMeasurements(compiled.silhouette);
				}
			} else if (needsMeasuredBounds) {
				const measurements = buildPuppetAlphaBounds(
					{ width: srcW, height: srcH, data: img },
					{ sampleResolution: maxDim }
				);
				if (measurements) applySilhouetteMeasurements(measurements);
			}
		}
	} catch (err) {
		console.warn("Failed to resolve puppet silhouette measurements:", err);
	}
	const createArtFin = (artZ: number[]) => {
		const ribbons = artFinMetadata?.silhouette.ribbons ?? [];
		if (!ribbons.length) return;
		const minZ = Math.min(...artZ);
		const maxZ = Math.max(...artZ);
		ribbons.forEach((ribbon, index) => {
			const normalizedPath = normalizePuppetRenderRibbonPath(ribbon.path);
			if (!normalizedPath) {
				console.warn(
					`Skipping invalid puppet silhouette ribbon for ${puppetId} at index ${index}.`
				);
				return;
			}
			const material = createFinMaterial(
				scene,
				`puppet_fin_mat_${puppetId}_${index}`,
				color3FromRenderColor(ribbon.edgeColor)
			);
			const fin = createRibbonFin({
				scene,
				name: `puppet_fin_${puppetId}_${index}`,
				parent: artNode,
				path: normalizedPath.map((point) => new Vector3(point[0], point[1], 0)),
				frontZ: minZ,
				backZ: maxZ,
				material,
				renderingGroupId: 2
			});
			if (!fin) material.dispose();
		});
	};

	if (mode === "flat") {
		const { artZ } = resolvePuppetDepthLayout(mode);
		const flatMat = new PBRMaterial(`avatarMat_flat_${puppetId}`, scene);
		flatMat.albedoTexture = avatarTexture;
		flatMat.unlit = true;
		flatMat.roughness = 0;
		flatMat.metallic = 0;
		flatMat.transparencyMode = PBRMaterial.PBRMATERIAL_ALPHATESTANDBLEND;
		flatMat.backFaceCulling = false;

		const planeWidth = PUPPET_ART_PLANE_SIZE;
		const planeHeight = PUPPET_ART_PLANE_SIZE;
		const createFlatPlane = (name: string, z: number, index: number) => {
			const plane = MeshBuilder.CreatePlane(
				name,
				{ width: planeWidth, height: planeHeight, sideOrientation: Mesh.DOUBLESIDE },
				scene
			);
			plane.parent = artNode;
			plane.position.z = z;
			plane.renderingGroupId = 2;
			plane.material = flatMat;
			plane.metadata = {
				...(plane.metadata ?? {}),
				bumblebeePuppetPart: "art-plane",
				bumblebeePuppetArtPlaneIndex: index,
				bumblebeePuppetArtPlaneCount: artZ.length,
				...(index === 0 ? { bumblebeePuppetArtPlaneEdge: "front" as const } : {}),
				...(index === artZ.length - 1 ? { bumblebeePuppetArtPlaneEdge: "back" as const } : {})
			} satisfies PuppetArtPlaneMetadata;
			return plane;
		};
		artZ.forEach((z, index) => createFlatPlane(`puppet_art_flat_${puppetId}_${index}`, z, index));
		createCircleDimOverlay(Math.max(...artZ) + PUPPET_PLANE_GAP);
		const fallbackMeasurements = buildCenteredArtPlaneMeasurements(planeWidth, planeHeight);
		return {
			node: artNode,
			visibleHeight: measuredVisibleHeight ?? fallbackMeasurements.visibleHeight,
			visibleArtBounds: measuredVisibleArtBounds ?? fallbackMeasurements.visibleArtBounds
		};
	}

	const { artZ } = resolvePuppetDepthLayout(mode);
	const frontMat = new PBRMaterial(`avatarMat_front_${puppetId}`, scene);
	frontMat.albedoTexture = avatarTexture;
	frontMat.unlit = true;
	frontMat.roughness = 0;
	frontMat.metallic = 0;
	frontMat.transparencyMode = PBRMaterial.PBRMATERIAL_ALPHATESTANDBLEND;
	frontMat.alphaCutOff = 0.35;
	// Render both sides to ensure no see-through from oblique angles
	frontMat.backFaceCulling = false;

	// Interior material: alpha test only to reduce overdraw
	const interiorMat = new PBRMaterial(`avatarMat_interior_${puppetId}`, scene);
	interiorMat.albedoTexture = avatarTexture;
	interiorMat.unlit = true;
	interiorMat.roughness = 0;
	interiorMat.metallic = 0;
	interiorMat.transparencyMode = PBRMaterial.PBRMATERIAL_ALPHATESTANDBLEND;
	interiorMat.alphaCutOff = 0.5;
	// Render both sides of interior shells so semi-transparent gradients (e.g., wand glow)
	// look correct through the stacked slices
	interiorMat.backFaceCulling = false;

	for (let i = 0; i < artZ.length; i++) {
		const z = artZ[i];
		const plane = MeshBuilder.CreatePlane(
			`puppet_art_${puppetId}_${i}`,
			{ size: PUPPET_ART_PLANE_SIZE, sideOrientation: Mesh.DOUBLESIDE },
			scene
		);
		plane.parent = artNode;
		plane.position.z = z;
		plane.renderingGroupId = 2;
		plane.metadata = {
			...(plane.metadata ?? {}),
			bumblebeePuppetPart: "art-plane",
			bumblebeePuppetArtPlaneIndex: i,
			bumblebeePuppetArtPlaneCount: artZ.length,
			...(i === 0 ? { bumblebeePuppetArtPlaneEdge: "front" as const } : {}),
			...(i === artZ.length - 1 ? { bumblebeePuppetArtPlaneEdge: "back" as const } : {})
		} satisfies PuppetArtPlaneMetadata;
		if (i === 0) {
			plane.material = frontMat;
			continue;
		}
		if (i === artZ.length - 1) {
			plane.material = frontMat;
			continue;
		}
		plane.material = interiorMat;
	}
	createCircleDimOverlay(Math.min(...artZ) - PUPPET_PLANE_GAP);
	if (shouldCreatePuppetArtFin(mode)) createArtFin(artZ);
	return {
		node: artNode,
		visibleHeight: measuredVisibleHeight,
		visibleArtBounds: measuredVisibleArtBounds
	};
}

//------------------------------------------------------------------------------
// Main function: Create complete 2D puppet
//------------------------------------------------------------------------------
const loadModel = async (options: Options) => {
	const {
		puppetId,
		imageUrl,
		imageMask,
		scene: rawScene,
		stickColor,
		renderMode,
		textureResolution,
		renderMetadata
	} = OptionsSchema.parse(options);
	const scene = rawScene as SceneWithCache;
	const templateKey = puppetTemplateKey({
		puppetId,
		imageUrl,
		imageMask,
		stickColor,
		renderMode,
		textureResolution,
		renderMetadata
	});
	const templateCache = getTemplateCache(scene);
	const cachedTemplate = getCachedTemplate(templateCache, templateKey);
	if (cachedTemplate) {
		return cloneTemplate(cachedTemplate, scene);
	}

	// Create main puppet node positioned at bottom of stick for proper pivoting
	const puppetNode = new TransformNode(`puppet_${puppetId}_${Date.now()}`, scene);

	// Hide puppet initially to prevent flash at world origin before positioning
	puppetNode.setEnabled(false);

	// Create an inner content node so we can animate subtle wobbles independently
	const contentNode = new TransformNode(`puppet_content_${puppetId}`, scene);
	contentNode.parent = puppetNode;

	// Create and clone stick mesh into uniform slices across thickness for apparent volume
	const baseStickMesh = await ensureStickMesh(scene, stickColor);
	const { stickZ } = resolvePuppetDepthLayout(renderMode);
	const stickSlices: Mesh[] = [];
	for (let i = 0; i < stickZ.length; i++) {
		const slice = baseStickMesh.clone(`stick_${puppetId}_${Date.now()}_${i}`, puppetNode) as Mesh;
		slice.setEnabled(true);
		slice.position.z = stickZ[i];
		stickSlices.push(slice);
	}
	if (stickSlices.length > 0) {
		const mergedStick = Mesh.MergeMeshes(stickSlices, true, true, undefined, false, true);
		if (mergedStick) {
			mergedStick.name = `stick_${puppetId}_merged`;
			mergedStick.parent = contentNode;
			mergedStick.material = baseStickMesh.material;
			mergedStick.renderingGroupId = 1;
		}
	}
	if (stickZ.length > 1) {
		const woodMat = baseStickMesh.material as StandardMaterial | undefined;
		const material = createFinMaterial(
			scene,
			`stick_edge_${puppetId}`,
			Color3.FromHexString(stickColor),
			woodMat?.emissiveTexture
		);
		createRibbonFin({
			scene,
			name: `stick_fin_${puppetId}`,
			parent: contentNode,
			path: createStickOutlinePath(),
			frontZ: Math.min(...stickZ),
			backZ: Math.max(...stickZ),
			material,
			renderingGroupId: 1
		});
	}
	// Stick is positioned so its bottom is at puppetNode origin (0,0,0)

	// Create art planes and position at stick top
	const imageRingColor = imageMask === "circle" ? stickColor : null;
	const art = await createPuppetPlanes(
		puppetId,
		scene,
		renderMode,
		textureResolution,
		imageUrl,
		imageMask,
		imageRingColor,
		renderMetadata
	);
	art.node.parent = contentNode;
	// Art sits at the stick top while puppet-space measurements remain rooted at stick bottom.
	art.node.position.y = PUPPET_STICK_TOP_Y;

	// puppetNode origin is now at bottom of stick - rotations will naturally pivot from there

	// Apply measured visible height from silhouette if available
	if (art.visibleHeight && art.visibleHeight > 0) {
		puppetNode.metadata = {
			localDims: {
				w: 2,
				h: art.visibleHeight
			},
			visibleArtBounds: art.visibleArtBounds
		};
	}

	// Fallback: if silhouette failed, estimate from hierarchy bounds
	if (!puppetNode.metadata?.localDims) {
		let minX = Number.POSITIVE_INFINITY,
			maxX = Number.NEGATIVE_INFINITY,
			minY = Number.POSITIVE_INFINITY,
			maxY = Number.NEGATIVE_INFINITY;
		for (const m of puppetNode.getChildMeshes()) {
			const bb = m.getBoundingInfo().boundingBox;
			const vmin = bb.minimumWorld;
			const vmax = bb.maximumWorld;
			if (vmin.x < minX) minX = vmin.x;
			if (vmax.x > maxX) maxX = vmax.x;
			if (vmin.y < minY) minY = vmin.y;
			if (vmax.y > maxY) maxY = vmax.y;
		}
		const width = Math.max(0.0001, maxX - minX);
		const height = Math.max(0.0001, maxY - minY);
		puppetNode.metadata = {
			localDims: {
				w: width,
				h: height
			},
			visibleArtBounds: {
				left: -width / 2,
				right: width / 2,
				top: height,
				bottom: 0,
				width,
				height
			}
		};
	}

	// TransformNode doesn't have setBoundingInfo method - only Mesh objects do

	// No animation groups for simple 2D puppets
	const template: PuppetTemplate = {
		node: puppetNode,
		metadata: deepCloneMetadata(puppetNode.metadata ?? {}),
		liveClones: 0,
		evicted: false,
		disposed: false
	};
	setTemplateCacheEntry(templateCache, templateKey, template);
	return cloneTemplate(template, scene);
};

export { loadModel, resolvePuppetDepthLayout, shouldCreatePuppetArtFin };
