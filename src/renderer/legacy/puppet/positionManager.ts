import { Vector2, Vector3 } from "@babylonjs/core/Maths/math.vector";
import { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import { Scene } from "@babylonjs/core/scene";
import { Camera } from "@babylonjs/core/Cameras/camera";
import { resolveCanvasViewportSize, screenToWorld } from "../utils/screenToWorld";
import { bbLog } from "../utils/bbLog";

interface ViewportPosition {
	xPercent: number; // 0-1, position from left edge
	yPercent: number; // 0-1, position from bottom edge
	heightPercent: number; // 0-1, size as percentage of viewport height
}

export interface PuppetPosition {
	horizontalPercent: number; // runtime center position along the bottom edge, 0-100
}

export class PuppetPositionManager {
	private occlusionRatio = 0.25;
	private viewportTarget: ViewportPosition = {
		xPercent: 0.8,
		// Rest position places a portion of puppet height below the bottom edge
		// yPercent is relative to viewport height
		yPercent: -0.25 * 0.25,
		// Default apparent height as fraction of viewport height
		heightPercent: 0.25
	};

	constructor(
		private node: TransformNode,
		private scene: Scene
	) {
		this.setOcclusionRatio(this.occlusionRatio);
	}

	setViewportTarget(target: Partial<ViewportPosition>) {
		this.viewportTarget = { ...this.viewportTarget, ...target };
	}

	setPuppetPosition(position: PuppetPosition) {
		// Convert puppet position to viewport position (always bottom edge)
		const { horizontalPercent } = position;
		const percent = Math.min(Math.max(horizontalPercent, 0), 100) / 100;

		this.viewportTarget = {
			xPercent: percent,
			// Rest Y keeps occlusionRatio of puppet height below the bottom edge
			yPercent: -this.occlusionRatio * this.viewportTarget.heightPercent,
			heightPercent: this.viewportTarget.heightPercent
		};

		bbLog("PPosMgr", "setPuppetPosition", {
			x: +this.viewportTarget.xPercent.toFixed(3),
			y: +this.viewportTarget.yPercent.toFixed(3),
			h: +this.viewportTarget.heightPercent.toFixed(3)
		});
	}

	setScalePercentage(heightPercent: number) {
		// Clamp to sensible bounds [0.02, 1.0]
		const clamped = Math.max(0.02, Math.min(1.0, heightPercent));
		// Update height and sync rest Y to keep 25% of puppet height below bottom
		this.viewportTarget = {
			...this.viewportTarget,
			heightPercent: clamped,
			yPercent: -this.occlusionRatio * clamped
		};
		bbLog("PPosMgr", "setScale", {
			h: +clamped.toFixed(3),
			y: +this.viewportTarget.yPercent.toFixed(3)
		});
	}

	setOcclusionRatio(value: number) {
		const clamped = Math.max(0.1, Math.min(1, value));
		this.occlusionRatio = clamped;
		this.viewportTarget = {
			...this.viewportTarget,
			yPercent: -this.occlusionRatio * this.viewportTarget.heightPercent
		};
		bbLog("PPosMgr", "setOcclusion", {
			occ: +this.occlusionRatio.toFixed(3),
			y: +this.viewportTarget.yPercent.toFixed(3)
		});
	}

	getOcclusionRatio() {
		return this.occlusionRatio;
	}

	getCurrentScale() {
		return this.viewportTarget.heightPercent;
	}

	getXPercent() {
		return this.viewportTarget.xPercent;
	}

	getViewportTarget(): Readonly<ViewportPosition> {
		return this.viewportTarget;
	}

	getWorldPosition(overrides?: Partial<ViewportPosition>): Vector3 {
		const target = { ...this.viewportTarget, ...overrides };
		return this.calculateWorldPosition(target);
	}

	getWorldPositionForOcclusionRatio(value: number): Vector3 {
		const clamped = Math.max(0.1, Math.min(1, value));
		return this.getWorldPosition({
			yPercent: -clamped * this.viewportTarget.heightPercent
		});
	}

	// Tracking/lerp removed — we position deterministically via Babylon animations

	private calculateWorldPosition(target: ViewportPosition): Vector3 {
		const engine = this.scene.getEngine();
		const canvas = engine.getRenderingCanvas()!;

		// Use the actual canvas client size for screen-space math to
		// match screenToWorld's reference dimensions exactly.
		const { cssWidth: viewW, cssHeight: viewH } = resolveCanvasViewportSize(canvas, engine);

		// Calculate puppet bottom Y position (in viewport space).
		// Our puppet pivot is at its bottom; yPercent is measured from the
		// bottom of the viewport (negative when below the bottom edge).
		// screenToWorld expects the OBJECT CENTER in screen pixels, not the
		// bottom. Convert bottom to center by subtracting half of the puppet's
		// on-screen height.
		const puppetBottomYFromBottom = target.yPercent;

		const bottomYPx = viewH * (1 - puppetBottomYFromBottom);
		// Convert bottom Y to center Y in screen pixels (for a vertically-fit object)
		const puppetScreenHeightPx = viewH * target.heightPercent;
		const centerYPx = bottomYPx - puppetScreenHeightPx * 0.5;
		const screenPos = new Vector2(viewW * target.xPercent, centerYPx);

		const camera = this.scene.activeCamera as Camera;

		// screenToWorld returns the position that centers the object at screenPos.
		// Our node pivot is at the bottom; shift center world down by half the object height.
		const centerWorld = screenToWorld(
			this.node,
			screenPos,
			camera,
			canvas,
			target.heightPercent,
			undefined,
			undefined,
			"vertical"
		);
		const objH = this.node.metadata?.localDims?.h ?? 1;
		const world = centerWorld.subtract(new Vector3(0, objH * 0.5, 0));
		bbLog("PPosMgr", "calcWorld", {
			xP: +target.xPercent.toFixed(3),
			yP: +target.yPercent.toFixed(3),
			hP: +target.heightPercent.toFixed(3),
			vw: viewW,
			vh: viewH,
			bPx: Math.round(bottomYPx),
			cPx: Math.round(centerYPx),
			wy: +world.y.toFixed(3)
		});
		return world;
	}

	dispose() {}
}
