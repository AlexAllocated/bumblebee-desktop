import { afterEach, describe, expect, mock, test } from "bun:test";

import * as rendererModule from "../src/renderer";
import { createPoseFromSubject, subjectCircleForPose } from "../src/geometry";

const actualRendererModule = { ...rendererModule };

const pose = {
	anchor: { x: 120, y: 120 },
	tailVector: { x: 32, y: 0 },
	subjectRadius: 24
};

class ListenerTrackingTarget extends EventTarget {
	readonly listeners = new Map<string, Set<EventListenerOrEventListenerObject>>();

	override addEventListener(
		type: string,
		callback: EventListenerOrEventListenerObject | null,
		options?: AddEventListenerOptions | boolean
	) {
		if (callback) {
			const listeners = this.listeners.get(type) ?? new Set<EventListenerOrEventListenerObject>();
			listeners.add(callback);
			this.listeners.set(type, listeners);
		}
		super.addEventListener(type, callback, options);
	}

	override removeEventListener(
		type: string,
		callback: EventListenerOrEventListenerObject | null,
		options?: EventListenerOptions | boolean
	) {
		if (callback) this.listeners.get(type)?.delete(callback);
		super.removeEventListener(type, callback, options);
	}

	listenerCount(type: string) {
		return this.listeners.get(type)?.size ?? 0;
	}
}

class FakeClassList {
	readonly names = new Set<string>();

	add(...names: string[]) {
		for (const name of names) this.names.add(name);
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

class FakeElement extends ListenerTrackingTarget {
	readonly classList = new FakeClassList();
	readonly style: Record<string, string> = {};
	readonly children: FakeElement[] = [];
	readonly dataset: Record<string, string> = {};
	parent: FakeElement | null = null;
	className = "";

	constructor(readonly tagName: string) {
		super();
	}

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

	setAttribute() {
		// Attribute values are not relevant to listener lifecycle coverage.
	}
}

class FakePointerEvent extends Event {
	readonly clientX: number;
	readonly clientY: number;

	constructor(type: string, init?: { clientX?: number; clientY?: number }) {
		super(type);
		this.clientX = init?.clientX ?? 0;
		this.clientY = init?.clientY ?? 0;
	}
}

const installDomHarness = () => {
	const previousDocument = Object.getOwnPropertyDescriptor(globalThis, "document");
	const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
	const previousPointerEvent = Object.getOwnPropertyDescriptor(globalThis, "PointerEvent");
	const target = new FakeElement("body");
	const windowTarget = new ListenerTrackingTarget() as ListenerTrackingTarget & {
		innerWidth: number;
		innerHeight: number;
	};
	windowTarget.innerWidth = 800;
	windowTarget.innerHeight = 600;
	const document = {
		body: target,
		createElement: (tagName: string) => new FakeElement(tagName),
		createElementNS: (_namespace: string, tagName: string) => new FakeElement(tagName)
	};

	Object.defineProperty(globalThis, "document", {
		configurable: true,
		value: document
	});
	Object.defineProperty(globalThis, "window", {
		configurable: true,
		value: windowTarget
	});
	Object.defineProperty(globalThis, "PointerEvent", {
		configurable: true,
		value: FakePointerEvent
	});

	return {
		target,
		windowTarget,
		restore() {
			for (const [key, descriptor] of [
				["document", previousDocument],
				["window", previousWindow],
				["PointerEvent", previousPointerEvent]
			] as const) {
				if (descriptor) Object.defineProperty(globalThis, key, descriptor);
				else delete (globalThis as Record<string, unknown>)[key];
			}
		}
	};
};

const deferred = () => {
	let resolve!: () => void;
	const promise = new Promise<void>((next) => {
		resolve = next;
	});
	return { promise, resolve };
};

afterEach(() => {
	mock.restore();
	// Bun keeps module mocks installed after mock.restore(), so explicitly put the
	// real renderer exports back before another test file imports them.
	mock.module("../src/renderer", () => actualRendererModule);
});

describe("speech bubble layer lifecycle", () => {
	test("tracks subject size changes without drifting the placed subject center", async () => {
		mock.module("../src/renderer", () => ({
			createBubbleRenderer: mock(() => ({
				element: {} as HTMLDivElement,
				debug: { overlapsSubject: false, exposedTailLength: 32 },
				update: mock(() => undefined),
				setPaused: mock(() => undefined),
				show: mock(() => undefined),
				hide: mock(() => Promise.resolve()),
				dispose: mock(() => undefined)
			})),
			measureBubbleBodySize: () => ({ width: 320, height: 128 }),
			resolveBubbleLayoutBounds: () => ({
				viewport: { left: 0, top: 0, width: 800, height: 600 },
				minWidthPx: 180,
				maxWidthPx: 320,
				maxHeightPx: 160
			})
		}));

		const { createBubbleLayer } = await import(`../src/layer.ts?test=${Date.now()}`);
		const layer = createBubbleLayer({
			target: {} as HTMLElement,
			viewport: { width: 800, height: 600 }
		});
		const initialPose = createPoseFromSubject({
			center: { x: 420, y: 320 },
			radius: 42,
			tailLength: 40,
			anchorAngle: 0
		});
		layer.show({ id: "bubble-a", text: "growing subject", pose: initialPose });

		const resizedPose = createPoseFromSubject({
			center: { x: 438, y: 306 },
			radius: 118,
			tailLength: 56,
			anchorAngle: 0
		});
		for (let index = 0; index < 120; index += 1) {
			layer.update("bubble-a", { pose: resizedPose });
		}

		const trackedSubject = subjectCircleForPose(layer.sessions[0]!.pose);
		expect(trackedSubject.center.x).toBeCloseTo(438, 2);
		expect(trackedSubject.center.y).toBeCloseTo(306, 2);
		expect(trackedSubject.radius).toBe(118);
	});

	test("repositions an active bubble when viewport clamping collapses its tail", async () => {
		let debug = {
			overlapsSubject: false,
			exposedTailLength: 32
		};
		const update = mock(() => {
			if (update.mock.calls.length === 2) {
				debug = { overlapsSubject: true, exposedTailLength: 0 };
			} else {
				debug = { overlapsSubject: false, exposedTailLength: 24 };
			}
		});

		mock.module("../src/renderer", () => ({
			createBubbleRenderer: mock(() => ({
				element: {} as HTMLDivElement,
				get debug() {
					return debug;
				},
				update,
				setPaused: mock(() => undefined),
				show: mock(() => undefined),
				hide: mock(() => Promise.resolve()),
				dispose: mock(() => undefined)
			})),
			measureBubbleBodySize: () => ({ width: 320, height: 128 }),
			resolveBubbleLayoutBounds: () => ({
				viewport: { left: 0, top: 0, width: 800, height: 600 },
				minWidthPx: 180,
				maxWidthPx: 320,
				maxHeightPx: 160
			})
		}));

		const { createBubbleLayer } = await import(`../src/layer.ts?test=${Date.now()}`);
		const layer = createBubbleLayer({
			target: {} as HTMLElement,
			viewport: { width: 800, height: 600 }
		});

		layer.show({ id: "bubble-a", text: "moving bubble", pose });
		layer.update("bubble-a", {
			pose: { ...pose, anchor: { x: 420, y: 280 } }
		});

		expect(update).toHaveBeenCalledTimes(3);
		expect(layer.sessions[0]?.pose).not.toEqual({ ...pose, anchor: { x: 420, y: 280 } });
	});

	test("a stale hide cannot remove a newer session with the same id", async () => {
		const hideResolvers: Array<() => void> = [];

		mock.module("../src/renderer", () => ({
			createBubbleRenderer: mock(() => {
				const hide = deferred();
				hideResolvers.push(hide.resolve);
				return {
					element: {} as HTMLDivElement,
					debug: null,
					update: mock(() => undefined),
					setPaused: mock(() => undefined),
					show: mock(() => undefined),
					hide: mock(() => hide.promise),
					dispose: mock(() => undefined)
				};
			}),
			measureBubbleBodySize: () => ({ width: 320, height: 128 }),
			resolveBubbleLayoutBounds: () => ({
				viewport: { left: 0, top: 0, width: 800, height: 600 },
				minWidthPx: 180,
				maxWidthPx: 320,
				maxHeightPx: 160
			})
		}));

		const { createBubbleLayer } = await import(`../src/layer.ts?test=${Date.now()}`);
		const layer = createBubbleLayer({
			target: {} as HTMLElement,
			viewport: { width: 800, height: 600 }
		});

		layer.show({ id: "bubble-a", text: "first bubble", pose });
		const hidePromise = layer.hide("bubble-a", { immediate: true });

		layer.show({ id: "bubble-a", text: "replacement bubble", pose });
		expect(layer.sessions).toEqual([
			expect.objectContaining({ id: "bubble-a", text: "replacement bubble", visible: true })
		]);

		hideResolvers[0]?.();
		await hidePromise;

		expect(layer.sessions).toEqual([
			expect.objectContaining({ id: "bubble-a", text: "replacement bubble", visible: true })
		]);
	});

	test("reveal cannot create a new chunk after a source starts hiding", async () => {
		const hide = deferred();

		mock.module("../src/renderer", () => ({
			createBubbleRenderer: mock(() => ({
				element: {} as HTMLDivElement,
				debug: null,
				update: mock(() => undefined),
				setPaused: mock(() => undefined),
				show: mock(() => undefined),
				hide: mock(() => hide.promise),
				dispose: mock(() => undefined)
			})),
			measureBubbleBodySize: () => ({ width: 320, height: 128 }),
			resolveBubbleLayoutBounds: () => ({
				viewport: { left: 0, top: 0, width: 800, height: 600 },
				minWidthPx: 180,
				maxWidthPx: 320,
				maxHeightPx: 160
			})
		}));

		const { createBubbleLayer } = await import(`../src/layer.ts?test=${Date.now()}`);
		const layer = createBubbleLayer({
			target: {} as HTMLElement,
			viewport: { width: 800, height: 600 }
		});

		layer.show({ id: "bubble-a", text: "first chunk second chunk", pose, manualReveal: true });
		const hidePromise = layer.hide("bubble-a", { immediate: true });

		layer.reveal("bubble-a", "second chunk", { chunkIndex: 1, fullCursor: 18 });

		expect(layer.sessions.map((session: { id: string }) => session.id)).toEqual(["bubble-a"]);
		expect(layer.sessions[0]?.visible).toBe(false);

		hide.resolve();
		await hidePromise;

		expect(layer.sessions).toEqual([]);
	});

	test("a page transition hides the previous page without waiting for the full reveal", async () => {
		const hides: ReturnType<typeof mock>[] = [];
		const elements: HTMLDivElement[] = [];
		const onPageExit = mock(() => undefined);

		mock.module("../src/renderer", () => ({
			createBubbleRenderer: mock(() => {
				const hide = mock(() => Promise.resolve());
				const element = {} as HTMLDivElement;
				hides.push(hide);
				elements.push(element);
				return {
					element,
					debug: null,
					update: mock(() => undefined),
					setPaused: mock(() => undefined),
					show: mock(() => undefined),
					hide,
					dispose: mock(() => undefined)
				};
			}),
			measureBubbleBodySize: () => ({ width: 320, height: 128 }),
			resolveBubbleLayoutBounds: () => ({
				viewport: { left: 0, top: 0, width: 800, height: 600 },
				minWidthPx: 180,
				maxWidthPx: 320,
				maxHeightPx: 160
			})
		}));

		const { createBubbleLayer } = await import(`../src/layer.ts?test=${Date.now()}`);
		const layer = createBubbleLayer({
			target: {} as HTMLElement,
			viewport: { width: 800, height: 600 },
			onPageExit
		});

		layer.show({
			id: "bubble-a",
			text: "first page second page",
			pose,
			manualReveal: true,
			animatePageExit: false
		});
		layer.reveal("bubble-a", "first page", {
			chunkIndex: 0,
			fullCursor: Number.NaN
		});
		expect(layer.getPage("bubble-a")).toMatchObject({
			id: "bubble-a",
			text: "first page",
			chunkIndex: 0,
			element: elements[0]
		});

		layer.reveal("bubble-a", "second page", {
			chunkIndex: 1,
			fullCursor: Number.NaN
		});

		expect(hides[0]).toHaveBeenCalledWith({ animate: false });
		expect(onPageExit).toHaveBeenCalledWith(
			expect.objectContaining({
				id: "bubble-a",
				sourceId: "bubble-a",
				text: "first page",
				chunkIndex: 0,
				element: elements[0]
			})
		);
		expect(layer.getPage("bubble-a")).toMatchObject({
			id: "bubble-a:fit-chunk:1",
			text: "second page",
			chunkIndex: 1,
			element: elements[1]
		});
		expect(layer.sessions).toEqual([
			expect.objectContaining({ id: "bubble-a", visible: false }),
			expect.objectContaining({ id: "bubble-a:fit-chunk:1", visible: true })
		]);
	});

	test("disposing a placeholder removes every drag listener it installed", async () => {
		const dom = installDomHarness();
		try {
			mock.module("../src/renderer", () => ({
				createBubbleRenderer: mock(() => ({
					element: new FakeElement("div") as unknown as HTMLDivElement,
					debug: null,
					update: mock(() => undefined),
					setPaused: mock(() => undefined),
					show: mock(() => undefined),
					hide: mock(() => Promise.resolve()),
					dispose: mock(() => undefined)
				})),
				resolveBubbleLayoutBounds: () => ({
					viewport: { left: 0, top: 0, width: 800, height: 600 },
					minWidthPx: 180,
					maxWidthPx: 320,
					maxHeightPx: 160
				})
			}));

			const { createBubblePlaceholder } = await import(`../src/placeholder.ts?test=${Date.now()}`);
			const placeholder = createBubblePlaceholder({
				target: dom.target as unknown as HTMLElement,
				pose,
				fixedBodySize: { width: 200, height: 80 },
				fixedBodyRect: { left: 90, top: 80, width: 200, height: 80 }
			});
			const moveHandle = dom.target.children.find((child) =>
				child.classList.contains("hive-speech-bubble-placeholder__subject-move")
			);
			expect(moveHandle).toBeDefined();
			expect(moveHandle?.listenerCount("pointerdown")).toBe(1);

			moveHandle?.dispatchEvent(
				new FakePointerEvent("pointerdown", { clientX: 120, clientY: 120 })
			);
			expect(dom.windowTarget.listenerCount("pointermove")).toBe(1);
			expect(dom.windowTarget.listenerCount("pointerup")).toBe(1);

			placeholder.dispose();

			expect(moveHandle?.listenerCount("pointerdown")).toBe(0);
			expect(dom.windowTarget.listenerCount("pointermove")).toBe(0);
			expect(dom.windowTarget.listenerCount("pointerup")).toBe(0);
		} finally {
			dom.restore();
		}
	});
});
