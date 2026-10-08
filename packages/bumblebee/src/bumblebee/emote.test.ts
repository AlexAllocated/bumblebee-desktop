import { describe, expect, test } from "bun:test";
import { resolveBumblebeeEmoteLoopRange } from "./emote";

describe("Bumblebee emote playback", () => {
	test("loops the held mad pose without replaying its neutral transitions", () => {
		expect(resolveBumblebeeEmoteLoopRange("mad")).toEqual({ from: 10, to: 11 });
	});

	test("leaves emotes without authored hold regions on their full range", () => {
		expect(resolveBumblebeeEmoteLoopRange("smileEyes")).toBeUndefined();
	});
});
