import { describe, expect, test } from "bun:test";
import { createMoveScrollFollower } from "./moveScrollFollower";

type ScrollCall = { left?: number; top?: number; behavior?: ScrollBehavior };

const scrollFixture = ({
	left = 0,
	top = 1200,
	right = 100,
	bottom = 1300,
	scrollX = 0,
	scrollY = 0,
	innerWidth = 400,
	innerHeight = 600,
	scrollWidth = 400,
	scrollHeight = 2400
} = {}) => {
	const calls: ScrollCall[] = [];
	const view = {
		scrollX,
		scrollY,
		innerWidth,
		innerHeight,
		scrollTo: (options: ScrollCall) => calls.push(options)
	};
	const document = {
		defaultView: view,
		documentElement: {
			clientWidth: innerWidth,
			clientHeight: innerHeight,
			scrollWidth,
			scrollHeight
		},
		body: { scrollWidth, scrollHeight }
	};
	const destination = {
		ownerDocument: document,
		getBoundingClientRect: () => ({ left, top, right, bottom })
	} as unknown as HTMLElement;
	return { calls, destination };
};

describe("moveTo scroll follower", () => {
	test("is disabled unless a move opts into page scrolling", () => {
		const { destination } = scrollFixture();

		expect(createMoveScrollFollower(destination, undefined)).toBeNull();
		expect(createMoveScrollFollower(destination, false)).toBeNull();
	});

	test("lets Bumblebee begin flying before the document follows", () => {
		const { calls, destination } = scrollFixture();
		const follower = createMoveScrollFollower(destination, true);

		expect(follower?.willScroll).toBe(true);
		follower?.update(0.1);
		follower?.update(0.2);
		expect(calls).toEqual([]);

		follower?.update(0.5);
		expect(calls).toHaveLength(1);
		expect(calls[0]?.top).toBeGreaterThan(0);
		expect(calls[0]?.top).toBeLessThan(950);
	});

	test("centers the destination at the end of the flight", () => {
		const { calls, destination } = scrollFixture();
		const follower = createMoveScrollFollower(destination, true);

		follower?.update(1);

		expect(calls).toEqual([{ left: 0, top: 950, behavior: "auto" }]);
	});

	test("supports explicit block alignment and clamps to document bounds", () => {
		const { calls, destination } = scrollFixture({ top: 2250, bottom: 2350 });
		const follower = createMoveScrollFollower(destination, { block: "start", inline: "nearest" });

		follower?.update(1);

		expect(calls).toEqual([{ left: 0, top: 1800, behavior: "auto" }]);
	});

	test("does not scroll when nearest alignment already exposes the destination", () => {
		const { calls, destination } = scrollFixture({ top: 100, bottom: 200 });
		const follower = createMoveScrollFollower(destination, {
			block: "nearest",
			inline: "nearest"
		});

		expect(follower?.willScroll).toBe(false);
		follower?.update(1);
		expect(calls).toEqual([]);
	});
});
