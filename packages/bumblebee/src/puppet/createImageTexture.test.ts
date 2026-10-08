import { describe, expect, test } from "bun:test";
import { resolvePuppetImageRequestPath } from "./createImageTexture";

describe("puppet image rendition selection", () => {
	test("selects the smallest rendition that satisfies the texture", () => {
		expect(resolvePuppetImageRequestPath("/puppet-image/polleen", 256)).toBe(
			"/puppet-image/polleen?width=256"
		);
		expect(resolvePuppetImageRequestPath("/puppet-image/polleen?v=abc", 900)).toBe(
			"/puppet-image/polleen?v=abc&width=1024"
		);
	});

	test("caps oversized textures and leaves external avatars unchanged", () => {
		expect(resolvePuppetImageRequestPath("/puppet-image/polleen", 4096)).toBe(
			"/puppet-image/polleen?width=2048"
		);
		expect(resolvePuppetImageRequestPath("https://cdn.example/avatar.png", 512)).toBe(
			"https://cdn.example/avatar.png"
		);
	});
});
