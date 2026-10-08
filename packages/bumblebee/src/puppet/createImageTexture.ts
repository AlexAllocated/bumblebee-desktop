import { DynamicTexture } from "@babylonjs/core/Materials/Textures/dynamicTexture";
import type { Scene } from "@babylonjs/core/scene";
import { BoundedPromiseCache } from "../utils/boundedPromiseCache";

const MAX_IMAGE_ELEMENT_CACHE_ENTRIES = 64;
const MAX_SCALED_CANVAS_CACHE_ENTRIES = 48;

const imageElementCache = new BoundedPromiseCache<HTMLImageElement>(
	MAX_IMAGE_ELEMENT_CACHE_ENTRIES
);
const scaledCanvasCache = new BoundedPromiseCache<{
	canvas: HTMLCanvasElement;
	width: number;
	height: number;
}>(MAX_SCALED_CANVAS_CACHE_ENTRIES);

type ImageMask = "none" | "circle";
const normalizeMask = (mask: unknown): ImageMask => (mask === "circle" ? "circle" : "none");
const normalizeRingColor = (value: unknown): string | null => {
	if (typeof value !== "string") return null;
	const trimmed = value.trim();
	return trimmed.length ? trimmed : null;
};

const PUPPET_RENDITION_WIDTHS = [256, 512, 1024, 2048] as const;
const resolvePuppetImageRequestPath = (imagePath: string, resolution: number) => {
	const base = "https://bumblebee.invalid";
	let url: URL;
	try {
		url = new URL(imagePath, base);
	} catch {
		return imagePath;
	}
	if (!url.pathname.startsWith("/puppet-image/")) return imagePath;
	const requested = Math.max(1, Math.ceil(resolution));
	const width =
		PUPPET_RENDITION_WIDTHS.find((candidate) => candidate >= requested) ??
		PUPPET_RENDITION_WIDTHS.at(-1)!;
	url.searchParams.set("width", String(width));
	return url.origin === base ? `${url.pathname}${url.search}${url.hash}` : url.toString();
};

const loadImageElement = (imagePath: string) => {
	let promise = imageElementCache.get(imagePath);
	if (!promise) {
		const promise = new Promise<HTMLImageElement>((resolve, reject) => {
			const image = new Image();
			image.crossOrigin = "anonymous";
			image.onload = () => {
				if (typeof image.decode === "function") {
					void image.decode().finally(() => resolve(image));
				} else {
					resolve(image);
				}
			};
			image.onerror = () => reject(new Error(`Failed to load image: ${imagePath}`));
			image.src = imagePath;
		});
		imageElementCache.set(imagePath, promise);
		return promise;
	}
	return promise;
};

const getScaledCanvas = async (
	imagePath: string,
	resolution: number,
	options?: { mask?: ImageMask; ringColor?: string | null }
) => {
	const mask = normalizeMask(options?.mask);
	const ringColor = normalizeRingColor(options?.ringColor);
	const ringKey = ringColor ? `__ring_${ringColor.toLowerCase()}` : "";
	const cacheKey = `${imagePath}__${resolution}__${mask}${ringKey}`;
	let promise = scaledCanvasCache.get(cacheKey);
	if (!promise) {
		const promise = (async () => {
			const image = await loadImageElement(imagePath);
			const scale = Math.min(1, resolution / image.width, resolution / image.height);
			const width = Math.max(1, Math.floor(image.width * scale));
			const height = Math.max(1, Math.floor(image.height * scale));
			const canvas = document.createElement("canvas");
			canvas.width = width;
			canvas.height = height;
			const context2D = canvas.getContext("2d");
			if (!context2D) {
				throw new Error("Failed to acquire 2D rendering context");
			}
			context2D.clearRect(0, 0, width, height);
			context2D.drawImage(image, 0, 0, width, height);

			if (mask === "circle") {
				// Apply an alpha mask like Discord's circular avatar crop.
				// Add a bit of transparent padding so the non-transparent pixels don't touch
				// the texture edge. This avoids silhouette/contour artifacts when sampling alpha.
				const minDim = Math.min(width, height);
				const borderPx = Math.max(1, Math.round(minDim * 0.0225));
				const pad = Math.max(borderPx + 2, Math.round(minDim * 0.06));
				const r = Math.max(1, minDim * 0.5 - pad);
				context2D.globalCompositeOperation = "destination-in";
				context2D.beginPath();
				context2D.arc(width / 2, height / 2, r, 0, Math.PI * 2);
				context2D.closePath();
				context2D.fill();

				// Optional ring (used for Discord avatar puppets) in the speaker's handle color.
				// We clip/composite so we do not re-introduce pixels outside the masked circle
				// (which would bring back silhouette artifacts).
				if (ringColor) {
					context2D.globalCompositeOperation = "source-atop";
					context2D.lineWidth = borderPx;
					context2D.strokeStyle = ringColor;
					context2D.lineCap = "round";
					context2D.lineJoin = "round";
					context2D.beginPath();
					context2D.arc(width / 2, height / 2, Math.max(1, r - borderPx / 2), 0, Math.PI * 2);
					context2D.closePath();
					context2D.stroke();
				}

				context2D.globalCompositeOperation = "source-over";
			}
			return { canvas, width, height };
		})();
		scaledCanvasCache.set(cacheKey, promise);
		return promise;
	}
	return promise;
};

const createImageTexture = async (
	name: string,
	imagePath: string,
	scene: Scene,
	resolution: number = 1024,
	options?: { mask?: ImageMask; ringColor?: string | null }
) => {
	const optimizedImagePath = resolvePuppetImageRequestPath(imagePath, resolution);
	const { canvas, width, height } = await getScaledCanvas(optimizedImagePath, resolution, options);
	const dynamicTexture = new DynamicTexture(name, { width, height }, scene, false);
	const dynamicContext = dynamicTexture.getContext();
	dynamicContext.clearRect(0, 0, width, height);
	dynamicContext.drawImage(canvas, 0, 0, width, height);
	dynamicTexture.hasAlpha = true;
	dynamicTexture.update();
	return dynamicTexture;
};

export { createImageTexture, resolvePuppetImageRequestPath };
