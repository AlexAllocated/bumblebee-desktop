import type { NameplateStyleTokens } from "../types";
import { normalizeClassToken, normalizeCssColor, normalizeRequiredClassToken } from "./styleTokens";

export type NameplatePlatform = "twitch" | "youtube" | "discord";

export type NameplateAnchor = {
	x: number;
	y: number;
	scale?: number;
	rotationDeg?: number;
	visible?: boolean;
	constrainToViewport?: boolean;
};

export type NameplatePayload = {
	text: string;
	style?: NameplateStyleTokens | null;
	platform?: NameplatePlatform | null;
};

export type NameplateOptions = {
	enabled?: boolean;
	zIndex?: number;
	container?: HTMLElement | null;
	scale?: number;
	designViewport?: { width: number; height: number } | null;
};

const DEFAULT_Z_INDEX = 2147483001;
const VIEWPORT_MARGIN_PX = 6;
const FALLBACK_NAMEPLATE_WIDTH_PX = 160;
const FALLBACK_NAMEPLATE_HEIGHT_PX = 36;
const DEFAULT_NAMEPLATE_SCALE = 1;

export const DEFAULT_NAMEPLATE_STYLE: NameplateStyleTokens = {
	background: "#ffffff",
	border: "#d1d5db",
	text: "#111827",
	accent: "#10b981",
	shadow: "rgba(17, 24, 39, 0.16)",
	fontFamily: "rounded",
	shape: "capsule",
	decoration: "shine"
};

const PLATFORM_ICON_PATHS: Record<
	NameplatePlatform,
	{ title: string; path: string; color: string }
> = {
	discord: {
		title: "Discord",
		color: "#5865F2",
		path: "M20.317 4.3698a19.7913 19.7913 0 00-4.8851-1.5152.0741.0741 0 00-.0785.0371c-.211.3753-.4447.8648-.6083 1.2495-1.8447-.2762-3.68-.2762-5.4868 0-.1636-.3933-.4058-.8742-.6177-1.2495a.077.077 0 00-.0785-.037 19.7363 19.7363 0 00-4.8852 1.515.0699.0699 0 00-.0321.0277C.5334 9.0458-.319 13.5799.0992 18.0578a.0824.0824 0 00.0312.0561c2.0528 1.5076 4.0413 2.4228 5.9929 3.0294a.0777.0777 0 00.0842-.0276c.4616-.6304.8731-1.2952 1.226-1.9942a.076.076 0 00-.0416-.1057c-.6528-.2476-1.2743-.5495-1.8722-.8923a.077.077 0 01-.0076-.1277c.1258-.0943.2517-.1923.3718-.2914a.0743.0743 0 01.0776-.0105c3.9278 1.7933 8.18 1.7933 12.0614 0a.0739.0739 0 01.0785.0095c.1202.099.246.1981.3728.2924a.077.077 0 01-.0066.1276 12.2986 12.2986 0 01-1.873.8914.0766.0766 0 00-.0407.1067c.3604.698.7719 1.3628 1.225 1.9932a.076.076 0 00.0842.0286c1.961-.6067 3.9495-1.5219 6.0023-3.0294a.077.077 0 00.0313-.0552c.5004-5.177-.8382-9.6739-3.5485-13.6604a.061.061 0 00-.0312-.0286zM8.02 15.3312c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9555-2.4189 2.157-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.9555 2.4189-2.1569 2.4189zm7.9748 0c-1.1825 0-2.1569-1.0857-2.1569-2.419 0-1.3332.9554-2.4189 2.1569-2.4189 1.2108 0 2.1757 1.0952 2.1568 2.419 0 1.3332-.946 2.4189-2.1568 2.4189Z"
	},
	twitch: {
		title: "Twitch",
		color: "#9146FF",
		path: "M11.571 4.714h1.715v5.143H11.57zm4.715 0H18v5.143h-1.714zM6 0L1.714 4.286v15.428h5.143V24l4.286-4.286h3.428L22.286 12V0zm14.571 11.143l-3.428 3.428h-3.429l-3 3v-3H6.857V1.714h13.714Z"
	},
	youtube: {
		title: "YouTube",
		color: "#FF0000",
		path: "M23.498 6.186a3.016 3.016 0 0 0-2.122-2.136C19.505 3.545 12 3.545 12 3.545s-7.505 0-9.377.505A3.017 3.017 0 0 0 .502 6.186C0 8.07 0 12 0 12s0 3.93.502 5.814a3.016 3.016 0 0 0 2.122 2.136c1.871.505 9.376.505 9.376.505s7.505 0 9.377-.505a3.015 3.015 0 0 0 2.122-2.136C24 15.93 24 12 24 12s0-3.93-.502-5.814zM9.545 15.568V8.432L15.818 12l-6.273 3.568z"
	}
};

const normalizeStyle = (style?: NameplateStyleTokens | null): NameplateStyleTokens => ({
	background: normalizeCssColor(style?.background, DEFAULT_NAMEPLATE_STYLE.background),
	border: normalizeCssColor(style?.border, DEFAULT_NAMEPLATE_STYLE.border),
	text: normalizeCssColor(style?.text, DEFAULT_NAMEPLATE_STYLE.text),
	accent: normalizeCssColor(style?.accent, DEFAULT_NAMEPLATE_STYLE.accent),
	shadow: normalizeCssColor(style?.shadow, DEFAULT_NAMEPLATE_STYLE.shadow),
	fontFamily: normalizeRequiredClassToken<NameplateStyleTokens["fontFamily"]>(
		style?.fontFamily,
		DEFAULT_NAMEPLATE_STYLE.fontFamily
	),
	shape: normalizeRequiredClassToken<NameplateStyleTokens["shape"]>(
		style?.shape,
		DEFAULT_NAMEPLATE_STYLE.shape
	),
	decoration: normalizeClassToken<NonNullable<NameplateStyleTokens["decoration"]>>(
		style?.decoration,
		DEFAULT_NAMEPLATE_STYLE.decoration
	),
	motion: normalizeClassToken<NonNullable<NameplateStyleTokens["motion"]>>(style?.motion)
});

const normalizePlatform = (platform: unknown): NameplatePlatform | null =>
	typeof platform === "string" && platform in PLATFORM_ICON_PATHS
		? (platform as NameplatePlatform)
		: null;

const createPlatformIcon = (platform?: NameplatePlatform | null) => {
	if (!platform) return null;
	const icon = PLATFORM_ICON_PATHS[platform];
	const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
	svg.classList.add("bumblebee-nameplate__icon");
	svg.setAttribute("role", "img");
	svg.setAttribute("viewBox", "0 0 24 24");
	svg.setAttribute("aria-label", icon.title);
	svg.style.color = icon.color;
	const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
	path.setAttribute("fill", "currentColor");
	path.setAttribute("d", icon.path);
	svg.append(path);
	return svg;
};

const renderNameplateContent = (element: HTMLElement, payload: NameplatePayload) => {
	const surface = document.createElement("span");
	surface.className = "bumblebee-nameplate__surface";
	const icon = createPlatformIcon(normalizePlatform(payload.platform));
	if (icon) surface.append(icon);
	const text = document.createElement("span");
	text.className = "bumblebee-nameplate__text";
	text.textContent = payload.text.trim();
	surface.append(text);
	element.replaceChildren(surface);
};

const finitePositive = (value: unknown): value is number =>
	typeof value === "number" && Number.isFinite(value) && value > 0;

const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));

const normalizeScale = (value: unknown) =>
	typeof value === "number" && Number.isFinite(value)
		? clamp(value, 0.2, 2.5)
		: DEFAULT_NAMEPLATE_SCALE;

const normalizeDesignViewport = (value: unknown) => {
	if (!value || typeof value !== "object") return undefined;
	const width = Number((value as { width?: unknown }).width);
	const height = Number((value as { height?: unknown }).height);
	return finitePositive(width) && finitePositive(height) ? { width, height } : undefined;
};

const nameplateFontSizePx = (viewportWidth: number) => clamp(viewportWidth * 0.015, 12, 18);

const getViewportSize = () => ({
	width:
		typeof window !== "undefined" && finitePositive(window.innerWidth) ? window.innerWidth : null,
	height:
		typeof window !== "undefined" && finitePositive(window.innerHeight) ? window.innerHeight : null
});

const getContainerGeometry = (container: HTMLElement | null) => {
	if (!container) return { left: 0, top: 0, ...getViewportSize() };
	const rect = container.getBoundingClientRect();
	return {
		left: Number.isFinite(rect.left) ? rect.left : 0,
		top: Number.isFinite(rect.top) ? rect.top : 0,
		width: finitePositive(rect.width) ? rect.width : null,
		height: finitePositive(rect.height) ? rect.height : null
	};
};

const getNameplateVisualSize = (element: HTMLElement, scale: number) => {
	const rect =
		typeof element.getBoundingClientRect === "function" ? element.getBoundingClientRect() : null;
	const width = finitePositive(element.offsetWidth)
		? element.offsetWidth * scale
		: finitePositive(rect?.width)
			? rect.width
			: FALLBACK_NAMEPLATE_WIDTH_PX * scale;
	const height = finitePositive(element.offsetHeight)
		? element.offsetHeight * scale
		: finitePositive(rect?.height)
			? rect.height
			: FALLBACK_NAMEPLATE_HEIGHT_PX * scale;
	return { width, height };
};

const clampNameplateToBounds = (
	element: HTMLElement,
	x: number,
	y: number,
	scale: number,
	bounds: Pick<ReturnType<typeof getContainerGeometry>, "width" | "height">
) => {
	if (!bounds.width && !bounds.height) return { x, y };
	const size = getNameplateVisualSize(element, scale);
	let nextX = x;
	let nextY = y;
	if (bounds.width) {
		const halfWidth = size.width / 2;
		const minX = VIEWPORT_MARGIN_PX + halfWidth;
		const maxX = bounds.width - VIEWPORT_MARGIN_PX - halfWidth;
		nextX = minX <= maxX ? Math.min(Math.max(nextX, minX), maxX) : bounds.width / 2;
	}
	if (bounds.height) {
		const maxY = Math.max(VIEWPORT_MARGIN_PX, bounds.height - VIEWPORT_MARGIN_PX - size.height);
		nextY = Math.min(Math.max(nextY, VIEWPORT_MARGIN_PX), maxY);
	}
	return { x: nextX, y: nextY };
};

export class Nameplates {
	/** Explicit offline clock; live nameplates never call this. */
	seek(timeMs: number) {
		for (const element of this.#elements.values()) {
			element.style.transition = "none";
			for (const animation of element.getAnimations({ subtree: true })) {
				animation.pause();
				animation.currentTime = Math.max(0, timeMs);
			}
		}
	}
	enabled: boolean;
	zIndex: number;
	scale: number;
	designViewport?: { width: number; height: number };
	#elements = new Map<string, HTMLElement>();
	#anchors = new Map<string, NameplateAnchor>();
	#container: HTMLElement | null;
	#paused = false;

	constructor(options?: NameplateOptions | boolean) {
		const resolved = typeof options === "boolean" ? { enabled: options } : (options ?? {});
		this.enabled = resolved.enabled ?? true;
		this.zIndex = Math.floor(resolved.zIndex ?? DEFAULT_Z_INDEX);
		this.scale = normalizeScale(resolved.scale);
		this.designViewport = normalizeDesignViewport(resolved.designViewport);
		this.#container = resolved.container ?? null;
	}

	show(id: string, payload: NameplatePayload, anchor: NameplateAnchor | null | undefined) {
		if (!this.enabled || !payload.text.trim() || !anchor) return;
		const element = this.#ensureElement(id);
		const style = normalizeStyle(payload.style);
		element.className = [
			"bumblebee-nameplate",
			this.#paused ? "is-playback-paused" : "",
			`bumblebee-nameplate--${style.shape}`,
			`bumblebee-nameplate--font-${style.fontFamily}`,
			style.decoration ? `bumblebee-nameplate--decoration-${style.decoration}` : "",
			style.motion ? `bumblebee-nameplate--motion-${style.motion}` : ""
		]
			.filter(Boolean)
			.join(" ");
		element.style.setProperty("--bb-nameplate-bg", style.background);
		element.style.setProperty("--bb-nameplate-border", style.border);
		element.style.setProperty("--bb-nameplate-text", style.text);
		element.style.setProperty("--bb-nameplate-accent", style.accent);
		element.style.setProperty("--bb-nameplate-shadow", style.shadow);
		element.style.zIndex = `${this.zIndex}`;
		this.#updateDesignTypography(element);
		renderNameplateContent(element, payload);
		this.updateGeometry(id, anchor);
		element.dataset.visible = "true";
	}

	setPaused(paused: boolean) {
		this.#paused = paused;
		for (const element of this.#elements.values()) {
			element.classList.toggle("is-playback-paused", paused);
		}
	}

	updateGeometry(id: string, anchor: NameplateAnchor | null | undefined) {
		const element = this.#elements.get(id);
		if (!element || !anchor) return;
		this.#anchors.set(id, anchor);
		const visible =
			anchor.visible !== false && Number.isFinite(anchor.x) && Number.isFinite(anchor.y);
		element.dataset.visible = visible ? "true" : "false";
		if (!visible) return;
		const rotationDeg =
			typeof anchor.rotationDeg === "number" && Number.isFinite(anchor.rotationDeg)
				? anchor.rotationDeg
				: 0;
		const anchorScale =
			typeof anchor.scale === "number" && Number.isFinite(anchor.scale)
				? Math.max(0.55, Math.min(1.65, anchor.scale))
				: 1;
		const container = getContainerGeometry(this.#container);
		const scale = this.#displayScale(anchorScale, container);
		const requestedPosition = {
			x: anchor.x - container.left,
			y: anchor.y - container.top + 6 * scale
		};
		const position =
			anchor.constrainToViewport === false
				? requestedPosition
				: clampNameplateToBounds(
						element,
						requestedPosition.x,
						requestedPosition.y,
						scale,
						container
					);
		element.style.transform = `translate3d(${Math.round(position.x)}px, ${Math.round(position.y)}px, 0) translateX(-50%) rotate(${rotationDeg.toFixed(2)}deg) scale(${scale.toFixed(3)})`;
	}

	hide(id: string) {
		const element = this.#elements.get(id);
		if (element) element.dataset.visible = "false";
		this.#anchors.delete(id);
	}

	remove(id: string) {
		this.#elements.get(id)?.remove();
		this.#elements.delete(id);
		this.#anchors.delete(id);
	}

	updateOptions(options?: NameplateOptions | boolean) {
		const resolved = typeof options === "boolean" ? { enabled: options } : (options ?? {});
		this.enabled = resolved.enabled ?? this.enabled;
		this.zIndex = Math.floor(resolved.zIndex ?? this.zIndex);
		if ("scale" in resolved) this.scale = normalizeScale(resolved.scale);
		if ("designViewport" in resolved) {
			this.designViewport = normalizeDesignViewport(resolved.designViewport);
		}
		if ("container" in resolved) this.setContainer(resolved.container);
		for (const element of this.#elements.values()) {
			element.style.zIndex = `${this.zIndex}`;
			this.#updateDesignTypography(element);
			if (!this.enabled) element.dataset.visible = "false";
		}
		for (const [id, anchor] of this.#anchors) this.updateGeometry(id, anchor);
	}

	setContainer(container?: HTMLElement | null) {
		const nextContainer = container ?? null;
		if (nextContainer === this.#container) return;
		this.#container = nextContainer;
		const target = this.#container ?? document.body;
		for (const element of this.#elements.values()) {
			element.style.position = this.#container ? "absolute" : "fixed";
			target.appendChild(element);
		}
	}

	dispose() {
		for (const element of this.#elements.values()) {
			element.remove();
		}
		this.#elements.clear();
		this.#anchors.clear();
	}

	#ensureElement(id: string) {
		const existing = this.#elements.get(id);
		if (existing) return existing;
		const element = document.createElement("div");
		element.style.position = this.#container ? "absolute" : "fixed";
		element.dataset.visible = "false";
		element.setAttribute("aria-live", "polite");
		(this.#container ?? document.body).appendChild(element);
		this.#elements.set(id, element);
		return element;
	}

	#displayScale(
		anchorScale: number,
		container: Pick<ReturnType<typeof getContainerGeometry>, "width" | "height">
	) {
		const authoredScale = anchorScale * this.scale;
		if (!this.designViewport || !container.width || !container.height) return authoredScale;
		return (
			authoredScale *
			Math.min(
				container.width / this.designViewport.width,
				container.height / this.designViewport.height
			)
		);
	}

	#updateDesignTypography(element: HTMLElement) {
		if (!this.designViewport) {
			element.style.removeProperty("--bb-nameplate-font-size");
			return;
		}
		element.style.setProperty(
			"--bb-nameplate-font-size",
			`${nameplateFontSizePx(this.designViewport.width)}px`
		);
	}
}
