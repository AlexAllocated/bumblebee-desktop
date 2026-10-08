import { describe, expect, test } from "bun:test";
import { localizeBubbleScreenRect } from "./chatBubbles";
import {
	createElementViewport,
	createSpeechPuppetOptions,
	findPuppetActorByTarget,
	resolveChatPuppetSurfaceOptions,
	resolveOverlayDecorationZIndexes
} from "./runtime";

describe("findPuppetActorByTarget", () => {
	test("keeps a directly configured puppet actor instead of applying remote-chat defaults", () => {
		const actor = { actorId: "puppet:polleen-demo", pose: { scale: 0.36, occlusion: 0.5 } };
		expect(
			findPuppetActorByTarget([actor], {
				type: "puppet",
				id: "polleen",
				instanceId: "polleen-demo",
				actorId: actor.actorId
			})
		).toBe(actor);
	});

	test("leaves remote puppet targets without an actor identity for default placement", () => {
		expect(
			findPuppetActorByTarget([{ actorId: "puppet:polleen" }], { type: "puppet", id: "polleen" })
		).toBeNull();
	});
});

describe("createElementViewport", () => {
	test("tracks the live local dimensions of a scoped decoration container", () => {
		let width = 960;
		const container = {
			getBoundingClientRect: () => ({ left: 120, top: 80, width, height: 540 })
		} as HTMLElement;
		const viewport = createElementViewport(container);

		expect(typeof viewport).toBe("function");
		expect((viewport as () => object)()).toEqual({ left: 0, top: 0, width: 960, height: 540 });
		width = 720;
		expect((viewport as () => object)()).toEqual({ left: 0, top: 0, width: 720, height: 540 });
	});
});

describe("localizeBubbleScreenRect", () => {
	test("translates projected screen geometry into the decoration container", () => {
		const container = {
			getBoundingClientRect: () => ({ left: 120, top: 80 })
		} as HTMLElement;

		expect(
			localizeBubbleScreenRect(
				{
					left: 620,
					right: 820,
					top: 300,
					bottom: 500,
					width: 200,
					height: 200,
					centerX: 720,
					centerY: 400
				},
				container
			)
		).toEqual({
			left: 500,
			right: 700,
			top: 220,
			bottom: 420,
			width: 200,
			height: 200,
			centerX: 600,
			centerY: 320
		});
	});
});

describe("createSpeechPuppetOptions", () => {
	test("applies chat puppet overlay defaults to runtime-created speech puppets", () => {
		const options = createSpeechPuppetOptions(
			{
				type: "puppet",
				id: "dandy",
				instanceId: "chatter:42",
				imageUrl: "https://example.com/puppet.png",
				imageMask: "circle",
				stickColor: "#facc15"
			},
			{
				position: { horizontalPercent: 32 },
				scale: 0.18,
				occlusion: 0.42
			}
		);

		expect(options).toEqual({
			instanceId: "chatter:42",
			imageUrl: "https://example.com/puppet.png",
			imageMask: "circle",
			stickColor: "#facc15",
			visible: false,
			position: { x: 0.32, y: 0.82 },
			scale: 0.18,
			occlusion: 0.42
		});
	});

	test("omits unspecified visual and placement fields when reusing an existing puppet", () => {
		const options = createSpeechPuppetOptions({ type: "puppet", id: "dandy" });

		expect(options).toEqual({ visible: false });
		expect("stickColor" in options).toBe(false);
	});
});

describe("resolveChatPuppetSurfaceOptions", () => {
	test("keeps puppets on the shared surface unless a dedicated layer is requested", () => {
		expect(resolveChatPuppetSurfaceOptions({ zIndex: 20 })).toBeNull();
	});

	test("inherits the main container while giving puppets their own foreground z-index", () => {
		const container = {} as HTMLElement;
		expect(
			resolveChatPuppetSurfaceOptions({
				surface: { container },
				zIndex: 20,
				chatPuppetZIndex: 35
			})
		).toEqual({
			surface: { container },
			zIndex: 35
		});
	});
});

describe("resolveOverlayDecorationZIndexes", () => {
	test("places bubbles and nameplates above a dedicated foreground puppet surface", () => {
		expect(resolveOverlayDecorationZIndexes({ zIndex: 20, chatPuppetZIndex: 35 })).toEqual({
			bubbles: 36,
			nameplates: 37
		});
	});

	test("keeps decoration defaults above the primary surface without a puppet layer", () => {
		expect(resolveOverlayDecorationZIndexes({ zIndex: 20 })).toEqual({
			bubbles: 21,
			nameplates: 22
		});
	});
});
