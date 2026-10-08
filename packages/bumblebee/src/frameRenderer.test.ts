import { expect, test } from "bun:test";
import { createFrameRenderer, type FrameActor, type FrameRendererOptions } from "./frameRenderer";
import { createBumblebeeTimelineBuilder } from "./timeline";

const actor: FrameActor = {
	id: "bee",
	kind: "bumblebee",
	modelUrl: "/bee.glb",
	timeline: createBumblebeeTimelineBuilder().build()
};
const options: FrameRendererOptions = {
	container: {} as HTMLElement,
	width: 1080,
	height: 1920,
	durationMs: 30000,
	actors: [actor]
};

test("reject invalid render contracts before allocating browser resources", () => {
	expect(() => createFrameRenderer({ ...options, width: Number.NaN })).toThrow(
		"positive and finite"
	);
	expect(() => createFrameRenderer({ ...options, actors: [actor, actor] })).toThrow("unique");
});

test("unprepared speech fails even when bubbles are disabled", () => {
	const timeline = createBumblebeeTimelineBuilder()
		.say({ id: "missing", text: "Not prepared" }, { bubbles: false })
		.build();
	expect(() => createFrameRenderer({ ...options, actors: [{ ...actor, timeline }] })).toThrow(
		"Speech missing is not prepared"
	);
});
