import { afterEach, describe, expect, test } from "bun:test";
import {
	MIN_EXPOSED_TAIL_LENGTH_PX,
	createPoseFromSubject,
	resolveTrackPlacement
} from "../src/geometry";
import { createBubbleRenderer, measureBubbleBodySize } from "../src/renderer";

const pose = {
	anchor: { x: 160, y: 140 },
	tailVector: { x: 36, y: 8 },
	subjectRadius: 24
};

class FakeClassList {
	readonly names = new Set<string>();

	add(...names: string[]) {
		for (const name of names) this.names.add(name);
	}

	remove(...names: string[]) {
		for (const name of names) this.names.delete(name);
	}

	contains(name: string) {
		return this.names.has(name);
	}

	toggle(name: string, force?: boolean) {
		const enabled = force ?? !this.names.has(name);
		if (enabled) this.names.add(name);
		else this.names.delete(name);
		return enabled;
	}
}

class FakeStyle {
	[key: string]: string | ((key: string, value: string) => void);

	setProperty(key: string, value: string) {
		this[key] = value;
	}
}

class FakeElement {
	readonly classList = new FakeClassList();
	readonly style = new FakeStyle();
	readonly children: FakeElement[] = [];
	readonly dataset: Record<string, string> = {};
	parent: FakeElement | null = null;
	className = "";
	textContent = "";

	constructor(readonly tagName: string) {}

	append(...children: FakeElement[]) {
		for (const child of children) {
			child.parent = this;
			this.children.push(child);
		}
	}

	remove() {
		if (!this.parent) return;
		this.parent.children.splice(this.parent.children.indexOf(this), 1);
		this.parent = null;
	}

	replaceChildren(...children: FakeElement[]) {
		for (const child of this.children) child.parent = null;
		this.children.splice(0, this.children.length);
		this.textContent = "";
		this.append(...children);
	}

	setAttribute() {
		// Attribute values are not relevant to lifecycle coverage.
	}
}

const installDom = () => {
	const previousDocument = Object.getOwnPropertyDescriptor(globalThis, "document");
	const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
	const previousRequestAnimationFrame = Object.getOwnPropertyDescriptor(
		globalThis,
		"requestAnimationFrame"
	);
	const previousCancelAnimationFrame = Object.getOwnPropertyDescriptor(
		globalThis,
		"cancelAnimationFrame"
	);
	const body = new FakeElement("body");
	const document = {
		body,
		createElement: (tagName: string) =>
			tagName === "canvas"
				? {
						getContext: () => ({
							font: "",
							measureText: (text: string) => ({ width: text.length * 13 })
						})
					}
				: new FakeElement(tagName),
		createElementNS: (_namespace: string, tagName: string) => new FakeElement(tagName)
	};
	const window = { innerWidth: 800, innerHeight: 600 };

	Object.defineProperty(globalThis, "document", {
		configurable: true,
		value: document
	});
	Object.defineProperty(globalThis, "window", {
		configurable: true,
		value: window
	});
	Object.defineProperty(globalThis, "requestAnimationFrame", {
		configurable: true,
		value: (callback: FrameRequestCallback) =>
			setTimeout(() => callback(performance.now()), 0) as unknown as number
	});
	Object.defineProperty(globalThis, "cancelAnimationFrame", {
		configurable: true,
		value: (handle: number) => clearTimeout(handle)
	});

	return {
		body,
		restore() {
			for (const [key, descriptor] of [
				["document", previousDocument],
				["window", previousWindow],
				["requestAnimationFrame", previousRequestAnimationFrame],
				["cancelAnimationFrame", previousCancelAnimationFrame]
			] as const) {
				if (descriptor) Object.defineProperty(globalThis, key, descriptor);
				else delete (globalThis as Record<string, unknown>)[key];
			}
		}
	};
};

let restoreDom: (() => void) | null = null;

afterEach(() => {
	restoreDom?.();
	restoreDom = null;
});

describe("speech bubble renderer lifecycle", () => {
	test("positions custom-target bubbles in the target's local coordinate space", () => {
		const dom = installDom();
		restoreDom = dom.restore;
		const target = new FakeElement("div");
		const renderer = createBubbleRenderer({
			target: target as unknown as HTMLElement,
			text: "scoped bubble",
			pose,
			viewport: { left: 0, top: 0, width: 640, height: 360 }
		});

		expect(renderer.element.style.position).toBe("absolute");
		renderer.dispose();
	});

	test("measures the bubble body without inflating it by the authored tail", () => {
		const dom = installDom();
		restoreDom = dom.restore;
		const longTailPose = {
			...pose,
			tailVector: { x: 180, y: 0 }
		};
		const bodySize = measureBubbleBodySize({
			text: "hello there",
			viewport: { left: 0, top: 0, width: 800, height: 600 }
		});
		const renderer = createBubbleRenderer({
			target: dom.body as unknown as HTMLElement,
			id: "bubble-a",
			text: "hello there",
			pose: longTailPose,
			viewport: { left: 0, top: 0, width: 800, height: 600 }
		});
		const debug = renderer.debug;
		expect(debug).not.toBeNull();
		if (!debug) {
			renderer.dispose();
			return;
		}

		expect(bodySize).toEqual({
			width: debug.contentRect.width,
			height: debug.contentRect.height
		});

		renderer.dispose();
	});

	test("preserves subject clearance after placing bubbles beside viewport edges", () => {
		const dom = installDom();
		restoreDom = dom.restore;
		const viewport = { left: 0, top: 0, width: 390, height: 640 };
		const text = "A viewport-constrained bubble should keep its tail visible.";
		const subjectCenters = [
			{ x: 52, y: 52 },
			{ x: 52, y: 320 },
			{ x: 52, y: 588 },
			{ x: 338, y: 52 },
			{ x: 338, y: 320 },
			{ x: 338, y: 588 },
			{ x: 195, y: 52 },
			{ x: 195, y: 588 }
		];

		for (const [index, center] of subjectCenters.entries()) {
			const authoredPose = createPoseFromSubject({
				center,
				radius: 42,
				tailLength: 32,
				anchorAngle: center.x > viewport.width / 2 ? 0 : Math.PI
			});
			const bodySize = measureBubbleBodySize({ text, viewport });
			const placement = resolveTrackPlacement({
				pose: authoredPose,
				viewport,
				estimatedSize: bodySize,
				random: () => 0
			});
			const renderer = createBubbleRenderer({
				target: dom.body as unknown as HTMLElement,
				id: `edge-bubble-${index}`,
				text,
				pose: placement.pose,
				viewport
			});

			expect(renderer.debug?.overlapsSubject).toBe(false);
			expect(renderer.debug?.exposedTailLength ?? 0).toBeGreaterThanOrEqual(
				MIN_EXPOSED_TAIL_LENGTH_PX
			);
			renderer.dispose();
		}
	});

	test("uses the lighter default bubble typography", () => {
		const dom = installDom();
		restoreDom = dom.restore;
		const renderer = createBubbleRenderer({
			target: dom.body as unknown as HTMLElement,
			id: "bubble-a",
			text: "hello there",
			pose,
			viewport: { left: 0, top: 0, width: 800, height: 600 }
		});

		const content = dom.body.children[0]?.children[2];
		const bubble = dom.body.children[0];
		expect(content?.style.fontWeight).toBe("550");
		expect(content?.style.fontSize).toBe("16px");
		expect(bubble?.className).toContain("shape-rounded");
		expect(bubble?.style["--bubble-body-radius"]).toBe("26px");

		renderer.dispose();
	});

	test("grows with revealed text and renders measured lines without browser wrapping", () => {
		const dom = installDom();
		restoreDom = dom.restore;
		const renderer = createBubbleRenderer({
			target: dom.body as unknown as HTMLElement,
			id: "bubble-a",
			text: "one",
			pose,
			minWidthPx: 180,
			maxWidthPx: 200,
			maxWidthPercent: 25,
			viewport: { left: 0, top: 0, width: 800, height: 600 }
		});

		const content = dom.body.children[0]?.children[2];
		const initialSize = { width: content?.style.width, height: content?.style.height };
		expect(content?.children.length).toBe(1);
		expect(
			content?.children
				.map((line) => line.textContent)
				.join(" ")
				.trim()
		).toBe("one");

		renderer.update({ text: "one two three four five six" });

		expect(content?.style.width).toBe(initialSize.width);
		expect(Number.parseFloat(String(content?.style.height ?? "0"))).toBeGreaterThan(
			Number.parseFloat(String(initialSize.height ?? "0"))
		);
		expect(content?.style.whiteSpace).toBe("pre");
		expect(
			content?.children
				.map((line) => line.textContent)
				.join(" ")
				.trim()
		).toBe("one two three four five six");

		renderer.dispose();
	});

	test("dispose settles an in-flight hide", async () => {
		const dom = installDom();
		restoreDom = dom.restore;
		const renderer = createBubbleRenderer({
			target: dom.body as unknown as HTMLElement,
			id: "bubble-a",
			text: "hello there",
			pose,
			viewport: { left: 0, top: 0, width: 800, height: 600 }
		});

		const hidePromise = renderer.hide();
		renderer.dispose();

		await hidePromise;
		expect(dom.body.children.length).toBe(0);
	});

	test("show settles the hide it interrupts", async () => {
		const dom = installDom();
		restoreDom = dom.restore;
		const renderer = createBubbleRenderer({
			target: dom.body as unknown as HTMLElement,
			id: "bubble-a",
			text: "hello there",
			pose,
			viewport: { left: 0, top: 0, width: 800, height: 600 }
		});

		const hidePromise = renderer.hide();
		renderer.show();

		await hidePromise;
		renderer.dispose();
		expect(dom.body.children.length).toBe(0);
	});

	test("can skip the exit animation for a visual handoff", async () => {
		const dom = installDom();
		restoreDom = dom.restore;
		const renderer = createBubbleRenderer({
			target: dom.body as unknown as HTMLElement,
			id: "bubble-a",
			text: "handoff",
			pose,
			viewport: { left: 0, top: 0, width: 800, height: 600 }
		});

		await renderer.hide({ animate: false });

		expect(renderer.element.classList.contains("leaving")).toBe(false);
		expect(renderer.element.style.opacity).toBe("0");

		renderer.dispose();
	});

	test("re-aims a viewport-constrained tail through the subject center", () => {
		const dom = installDom();
		restoreDom = dom.restore;
		const renderer = createBubbleRenderer({
			target: dom.body as unknown as HTMLElement,
			id: "bubble-a",
			text: "A bubble being migrated into a narrow CRT viewport",
			pose: {
				anchor: { x: 478.95, y: 270.8 },
				tailVector: { x: -38.04, y: -24.72 },
				subjectRadius: 42
			},
			minWidthPx: 220,
			maxWidthPx: 260,
			viewport: { left: 100, top: 80, width: 240, height: 240 }
		});

		const debug = renderer.debug;
		expect(debug).not.toBeNull();
		if (!debug) return;
		expect(debug.contentRect.right).toBeLessThanOrEqual(332);

		const tail = {
			x: debug.tailTip.x - debug.anchor.x,
			y: debug.tailTip.y - debug.anchor.y
		};
		const subject = {
			x: debug.subjectCircle.center.x - debug.anchor.x,
			y: debug.subjectCircle.center.y - debug.anchor.y
		};
		const crossProduct = tail.x * subject.y - tail.y * subject.x;
		const dotProduct = tail.x * subject.x + tail.y * subject.y;

		expect(Math.abs(crossProduct)).toBeLessThan(0.5);
		expect(dotProduct).toBeGreaterThan(0);
		expect(
			Math.hypot(
				debug.subjectCircle.center.x - debug.tailTip.x,
				debug.subjectCircle.center.y - debug.tailTip.y
			)
		).toBeCloseTo(debug.subjectCircle.radius, 1);

		renderer.dispose();
	});
});
