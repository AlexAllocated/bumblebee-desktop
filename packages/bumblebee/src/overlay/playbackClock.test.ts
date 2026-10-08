import { describe, expect, test } from "bun:test";
import { PlaybackClock } from "./playbackClock";

describe("PlaybackClock", () => {
	test("reports presentation time without wall time spent paused", async () => {
		const clock = new PlaybackClock();
		await Bun.sleep(8);
		clock.setPaused(true);
		const pausedAt = clock.now();
		await Bun.sleep(25);
		expect(clock.now()).toBe(pausedAt);

		clock.setPaused(false);
		await Bun.sleep(8);
		expect(clock.now() - pausedAt).toBeGreaterThanOrEqual(5);
		expect(clock.now() - pausedAt).toBeLessThan(25);
	});
});
