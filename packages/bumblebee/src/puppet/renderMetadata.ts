import { isoLines } from "marching-squares";
import type {
	PuppetRenderableMetadata,
	PuppetRenderMetadata,
	PuppetRenderPoint,
	PuppetSilhouetteMeasurements
} from "../types";
import { PUPPET_STICK_TOP_Y } from "./geometry";

type RgbaPixels = {
	width: number;
	height: number;
	data: Uint8ClampedArray | Uint8Array;
};

const MAX_RENDER_METADATA_SAMPLE_RESOLUTION = 2048;
const MAX_RENDER_POINT_MAGNITUDE = 1.01;
const MIN_RENDER_RIBBON_AREA = 1e-6;
const MAX_SEGMENT_OUTLIER_RATIO = 12;

const hashString = (value: string) => {
	let hash = 0x811c9dc5;
	for (let i = 0; i < value.length; i++) {
		hash ^= value.charCodeAt(i);
		hash = Math.imul(hash, 0x01000193);
	}
	return (hash >>> 0).toString(16);
};

const finiteNumberKey = (value: unknown) =>
	typeof value === "number" && Number.isFinite(value) ? Number(value.toFixed(6)) : null;

const renderBoundsKey = (bounds: PuppetSilhouetteMeasurements["visibleArtBounds"]) => ({
	left: finiteNumberKey(bounds.left),
	right: finiteNumberKey(bounds.right),
	top: finiteNumberKey(bounds.top),
	bottom: finiteNumberKey(bounds.bottom),
	width: finiteNumberKey(bounds.width),
	height: finiteNumberKey(bounds.height)
});

const renderColorKey = (
	color?: PuppetRenderMetadata["silhouette"]["ribbons"][number]["edgeColor"] | null
) => ({
	r: finiteNumberKey(color?.r),
	g: finiteNumberKey(color?.g),
	b: finiteNumberKey(color?.b)
});

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

const renderMetadataContentKey = (metadata?: PuppetRenderableMetadata | null) => {
	if (!metadata) return "none";
	if (isPuppetRenderMetadata(metadata)) {
		const content = {
			kind: "full",
			version: metadata.version,
			image: {
				width: finiteNumberKey(metadata.image.width),
				height: finiteNumberKey(metadata.image.height)
			},
			silhouette: {
				sampleResolution: finiteNumberKey(metadata.silhouette.sampleResolution),
				visibleHeight: finiteNumberKey(metadata.silhouette.visibleHeight),
				visibleArtBounds: renderBoundsKey(metadata.silhouette.visibleArtBounds),
				ribbons: metadata.silhouette.ribbons.map((ribbon) => ({
					edgeColor: renderColorKey(ribbon.edgeColor),
					path:
						normalizePuppetRenderRibbonPath(ribbon.path)?.map((point) => [
							finiteNumberKey(point[0]),
							finiteNumberKey(point[1])
						]) ?? null
				}))
			}
		};
		return hashString(JSON.stringify(content));
	}
	const content = {
		kind: "measurements",
		sampleResolution: finiteNumberKey(metadata.sampleResolution),
		visibleHeight: finiteNumberKey(metadata.visibleHeight),
		visibleArtBounds: renderBoundsKey(metadata.visibleArtBounds)
	};
	return hashString(JSON.stringify(content));
};

const pathLength = (path: number[][]) => {
	let sum = 0;
	for (let i = 1; i < path.length; i++) {
		const dx = path[i][0] - path[i - 1][0];
		const dy = path[i][1] - path[i - 1][1];
		sum += Math.hypot(dx, dy);
	}
	return sum;
};

const clampIndex = (value: number, max: number) => Math.min(max, Math.max(0, value));

const buildAlphaGrid = (pixels: RgbaPixels, sampleResolution: number) => {
	const sourceWidth = pixels.width;
	const sourceHeight = pixels.height;
	const maxDim = Math.max(
		128,
		Math.min(sampleResolution, MAX_RENDER_METADATA_SAMPLE_RESOLUTION, sourceWidth, sourceHeight)
	);
	const width = Math.min(maxDim, sourceWidth);
	const height = Math.min(maxDim, sourceHeight);
	const grid: number[][] = new Array(height);
	const stepX = sourceWidth / width;
	const stepY = sourceHeight / height;
	for (let y = 0; y < height; y++) {
		const row: number[] = new Array(width);
		const sourceY = clampIndex(Math.floor((y + 0.5) * stepY), sourceHeight - 1);
		for (let x = 0; x < width; x++) {
			const sourceX = clampIndex(Math.floor((x + 0.5) * stepX), sourceWidth - 1);
			row[x] = pixels.data[4 * (sourceY * sourceWidth + sourceX) + 3] / 255;
		}
		grid[y] = row;
	}
	return { grid, width, height, sampleResolution: maxDim };
};

const normalizeContourPath = (path: number[][]) => {
	if (path.length > 1) {
		const first = path[0];
		const last = path[path.length - 1];
		if (first[0] === last[0] && first[1] === last[1]) {
			return path.slice(0, -1);
		}
	}
	return path;
};

const pathArea = (path: readonly (readonly number[])[]) => {
	let sum = 0;
	for (let i = 0; i < path.length; i++) {
		const current = path[i];
		const next = path[(i + 1) % path.length];
		sum += current[0] * next[1] - next[0] * current[1];
	}
	return Math.abs(sum) / 2;
};

const normalizePuppetRenderRibbonPath = (value: unknown): PuppetRenderPoint[] | null => {
	if (!Array.isArray(value)) return null;
	const path: PuppetRenderPoint[] = [];
	for (const entry of value) {
		if (
			!Array.isArray(entry) ||
			entry.length !== 2 ||
			typeof entry[0] !== "number" ||
			typeof entry[1] !== "number" ||
			!Number.isFinite(entry[0]) ||
			!Number.isFinite(entry[1]) ||
			Math.abs(entry[0]) > MAX_RENDER_POINT_MAGNITUDE ||
			Math.abs(entry[1]) > MAX_RENDER_POINT_MAGNITUDE
		) {
			return null;
		}

		const point = [entry[0], entry[1]] as const;
		const previous = path.at(-1);
		if (previous && previous[0] === point[0] && previous[1] === point[1]) continue;
		path.push(point);
	}

	const first = path[0];
	const last = path.at(-1);
	if (path.length > 1 && first && last && first[0] === last[0] && first[1] === last[1]) {
		path.pop();
	}
	if (path.length < 3 || pathArea(path) < MIN_RENDER_RIBBON_AREA) return null;

	const segmentLengths = path.map((point, index) => {
		const next = path[(index + 1) % path.length]!;
		return Math.hypot(next[0] - point[0], next[1] - point[1]);
	});
	const sortedSegmentLengths = [...segmentLengths].sort((left, right) => left - right);
	const medianSegmentLength =
		sortedSegmentLengths[Math.floor(sortedSegmentLengths.length / 2)] ?? 0;
	if (
		medianSegmentLength <= 0 ||
		Math.max(...segmentLengths) > Math.max(0.5, medianSegmentLength * MAX_SEGMENT_OUTLIER_RATIO)
	) {
		return null;
	}

	return path;
};

const findContourPaths = (grid: number[][]) => {
	const contours = isoLines(grid, [0.5], { linearRing: true, noFrame: true });
	const paths = (Array.isArray(contours) && contours.length > 0 ? contours[0] : []) as
		number[][][] | [];
	return paths
		.map(normalizeContourPath)
		.filter((path) => path.length >= 3)
		.sort((left, right) => pathLength(right) - pathLength(left));
};

const sampleAlpha = (
	pixels: RgbaPixels,
	sampleX: number,
	sampleY: number,
	sampleWidth: number,
	sampleHeight: number
) => {
	const sourceX = clampIndex(
		Math.round((sampleX / Math.max(1, sampleWidth - 1)) * (pixels.width - 1)),
		pixels.width - 1
	);
	const sourceY = clampIndex(
		Math.round((sampleY / Math.max(1, sampleHeight - 1)) * (pixels.height - 1)),
		pixels.height - 1
	);
	return pixels.data[4 * (sourceY * pixels.width + sourceX) + 3] / 255;
};

const hasHardAlphaEdge = (
	pixels: RgbaPixels,
	path: number[][],
	sampleWidth: number,
	sampleHeight: number
) => {
	const maxSamples = 512;
	const step = Math.max(1, Math.floor(path.length / maxSamples));
	let checked = 0;
	let hard = 0;
	for (let i = 0; i < path.length; i += step) {
		const point = path[i];
		let minAlpha = 1;
		let maxAlpha = 0;
		for (let dy = -1; dy <= 1; dy++) {
			for (let dx = -1; dx <= 1; dx++) {
				const alpha = sampleAlpha(pixels, point[0] + dx, point[1] + dy, sampleWidth, sampleHeight);
				minAlpha = Math.min(minAlpha, alpha);
				maxAlpha = Math.max(maxAlpha, alpha);
			}
		}
		checked++;
		if (maxAlpha >= 0.94 && minAlpha <= 0.08) hard++;
	}
	return checked > 0 && hard / checked >= 0.25;
};

const findRibbonContourPaths = (
	pixels: RgbaPixels,
	grid: number[][],
	sampleWidth: number,
	sampleHeight: number
) => {
	const minArea = Math.max(4, sampleWidth * sampleHeight * 0.00002);
	const minLength = Math.max(8, Math.min(sampleWidth, sampleHeight) * 0.01);
	return findContourPaths(grid).filter(
		(path) =>
			pathArea(path) >= minArea &&
			pathLength(path) >= minLength &&
			hasHardAlphaEdge(pixels, path, sampleWidth, sampleHeight)
	);
};

const contourToPlanePoints = (path: number[][], width: number, height: number) =>
	path.map((point): PuppetRenderPoint => {
		const nx = point[0] / Math.max(1, width - 1);
		const ny = point[1] / Math.max(1, height - 1);
		return [-1 + 2 * nx, 1 - 2 * ny];
	});

const buildRibbonPath = (mapped: PuppetRenderPoint[]) => {
	return mapped;
};

const sampleEdgeColor = (
	pixels: RgbaPixels,
	chosen: number[][],
	sampleWidth: number,
	sampleHeight: number
) => {
	type ColorBucket = {
		count: number;
		saturationSum: number;
		rSum: number;
		gSum: number;
		bSum: number;
	};
	const createBucket = (): ColorBucket => ({
		count: 0,
		saturationSum: 0,
		rSum: 0,
		gSum: 0,
		bSum: 0
	});
	const addToBucket = (
		buckets: Map<number, ColorBucket>,
		key: number,
		r: number,
		g: number,
		b: number,
		saturation: number
	) => {
		const target = buckets.get(key) ?? createBucket();
		target.count += 1;
		target.saturationSum += saturation;
		target.rSum += r;
		target.gSum += g;
		target.bSum += b;
		buckets.set(key, target);
	};
	const chooseBestBucket = (buckets: Map<number, ColorBucket>) => {
		let bestBucket: ColorBucket | null = null;
		for (const bucket of buckets.values()) {
			if (
				!bestBucket ||
				bucket.count > bestBucket.count ||
				(bucket.count === bestBucket.count && bucket.saturationSum > bestBucket.saturationSum)
			) {
				bestBucket = bucket;
			}
		}
		return bestBucket;
	};
	const buckets = new Map<number, ColorBucket>();
	const fillBuckets = new Map<number, ColorBucket>();
	const maxSamples = 2048;
	const step = Math.max(1, Math.floor(chosen.length / maxSamples));
	for (let i = 0; i < chosen.length; i += step) {
		const point = chosen[i];
		const sx = clampIndex(
			Math.round((point[0] / Math.max(1, sampleWidth - 1)) * (pixels.width - 1)),
			pixels.width - 1
		);
		const sy = clampIndex(
			Math.round((point[1] / Math.max(1, sampleHeight - 1)) * (pixels.height - 1)),
			pixels.height - 1
		);
		for (let dy = -1; dy <= 1; dy++) {
			for (let dx = -1; dx <= 1; dx++) {
				const px = clampIndex(sx + dx, pixels.width - 1);
				const py = clampIndex(sy + dy, pixels.height - 1);
				const index = 4 * (py * pixels.width + px);
				if (pixels.data[index + 3] > 127) {
					const r = pixels.data[index];
					const g = pixels.data[index + 1];
					const b = pixels.data[index + 2];
					const bucketKey = ((r >> 4) << 8) | ((g >> 4) << 4) | (b >> 4);
					const max = Math.max(r, g, b);
					const min = Math.min(r, g, b);
					const saturation = max > 0 ? (max - min) / max : 0;
					const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
					addToBucket(buckets, bucketKey, r, g, b, saturation);
					if (!(luminance < 0.18 && saturation < 0.25)) {
						addToBucket(fillBuckets, bucketKey, r, g, b, saturation);
					}
				}
			}
		}
	}
	const bestBucket = chooseBestBucket(fillBuckets) ?? chooseBestBucket(buckets);
	return bestBucket
		? {
				r: bestBucket.rSum / bestBucket.count / 255,
				g: bestBucket.gSum / bestBucket.count / 255,
				b: bestBucket.bSum / bestBucket.count / 255
			}
		: { r: 0.1, g: 0.1, b: 0.1 };
};

const measureSilhouette = (
	mapped: PuppetRenderPoint[],
	sampleResolution: number
): PuppetSilhouetteMeasurements | null => {
	let minX = Number.POSITIVE_INFINITY;
	let maxX = Number.NEGATIVE_INFINITY;
	let minY = Number.POSITIVE_INFINITY;
	let maxY = Number.NEGATIVE_INFINITY;
	for (const point of mapped) {
		if (point[0] < minX) minX = point[0];
		if (point[0] > maxX) maxX = point[0];
		if (point[1] < minY) minY = point[1];
		if (point[1] > maxY) maxY = point[1];
	}
	if (![minX, maxX, minY, maxY].every(Number.isFinite)) return null;
	const visibleTopWorldY = PUPPET_STICK_TOP_Y + maxY;
	const visibleBottomWorldYForArt = PUPPET_STICK_TOP_Y + minY;
	const visibleHeight = Math.max(0.0001, visibleTopWorldY);
	return {
		sampleResolution,
		visibleArtBounds: {
			left: minX,
			right: maxX,
			top: visibleTopWorldY,
			bottom: visibleBottomWorldYForArt,
			width: Math.max(0.0001, maxX - minX),
			height: Math.max(0.0001, visibleTopWorldY - visibleBottomWorldYForArt)
		},
		visibleHeight
	};
};

const buildPuppetSilhouetteMeasurements = (
	pixels: RgbaPixels,
	options?: { sampleResolution?: number }
): PuppetSilhouetteMeasurements | null => {
	const sampleResolution = options?.sampleResolution ?? 1024;
	const {
		grid,
		width,
		height,
		sampleResolution: resolvedSampleResolution
	} = buildAlphaGrid(pixels, sampleResolution);
	const contours = findRibbonContourPaths(pixels, grid, width, height);
	if (!contours.length) return null;
	const mapped = contours.flatMap((path) => contourToPlanePoints(path, width, height));
	return measureSilhouette(mapped, resolvedSampleResolution);
};

const buildPuppetAlphaBounds = (
	pixels: RgbaPixels,
	options?: { sampleResolution?: number }
): PuppetSilhouetteMeasurements | null => {
	const sampleResolution = options?.sampleResolution ?? 1024;
	const {
		grid,
		width,
		height,
		sampleResolution: resolvedSampleResolution
	} = buildAlphaGrid(pixels, sampleResolution);
	let minX = Number.POSITIVE_INFINITY;
	let maxX = Number.NEGATIVE_INFINITY;
	let minY = Number.POSITIVE_INFINITY;
	let maxY = Number.NEGATIVE_INFINITY;
	for (let y = 0; y < height; y++) {
		for (let x = 0; x < width; x++) {
			if (grid[y][x] < 0.5) continue;
			if (x < minX) minX = x;
			if (x > maxX) maxX = x;
			if (y < minY) minY = y;
			if (y > maxY) maxY = y;
		}
	}
	if (![minX, maxX, minY, maxY].every(Number.isFinite)) return null;
	return measureSilhouette(
		contourToPlanePoints(
			[
				[minX, minY],
				[maxX, maxY]
			],
			width,
			height
		),
		resolvedSampleResolution
	);
};

const buildPuppetRenderMetadata = (
	pixels: RgbaPixels,
	options?: { sampleResolution?: number; generatedAt?: string }
): PuppetRenderMetadata | null => {
	const sampleResolution = options?.sampleResolution ?? 1024;
	const {
		grid,
		width,
		height,
		sampleResolution: resolvedSampleResolution
	} = buildAlphaGrid(pixels, sampleResolution);
	const contours = findRibbonContourPaths(pixels, grid, width, height);
	if (!contours.length) return null;
	const mappedPaths = contours.map((path) => contourToPlanePoints(path, width, height));
	const mapped = mappedPaths.flat();
	const measurements = measureSilhouette(mapped, resolvedSampleResolution);
	if (!measurements) return null;
	const ribbons = mappedPaths.map((mappedPath, index) => ({
		path: buildRibbonPath(mappedPath),
		edgeColor: sampleEdgeColor(pixels, contours[index], width, height)
	}));
	return {
		version: 2,
		image: {
			width: pixels.width,
			height: pixels.height
		},
		silhouette: {
			...measurements,
			ribbons
		},
		generatedAt: options?.generatedAt
	};
};

const metadataCacheKey = (imageUrl: string, imageMask: string, resolution: number) =>
	`${hashString(`${imageUrl}|${imageMask}`)}_${resolution}`;

export {
	buildPuppetAlphaBounds,
	buildPuppetRenderMetadata,
	buildPuppetSilhouetteMeasurements,
	metadataCacheKey,
	normalizePuppetRenderRibbonPath,
	renderMetadataContentKey
};
