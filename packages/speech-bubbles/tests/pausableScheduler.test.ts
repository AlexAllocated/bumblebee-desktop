import { describe, expect, test } from "bun:test";
import { PausableScheduler } from "../src/pausableScheduler";

describe("PausableScheduler", () => {
	test("preserves the remaining timeout duration across a pause", async () => {
		const scheduler = new PausableScheduler();
		let fired = false;
		scheduler.setTimeout(() => {
			fired = true;
		}, 40);

		await Bun.sleep(12);
		scheduler.setPaused(true);
		const pausedNow = scheduler.now();
		await Bun.sleep(55);

		expect(fired).toBe(false);
		expect(scheduler.now()).toBe(pausedNow);

		scheduler.setPaused(false);
		await Bun.sleep(12);
		expect(fired).toBe(false);
		await Bun.sleep(35);
		expect(fired).toBe(true);
		scheduler.dispose();
	});

	test("holds newly scheduled work until playback resumes", async () => {
		const scheduler = new PausableScheduler();
		scheduler.setPaused(true);
		let calls = 0;
		scheduler.setTimeout(() => {
			calls += 1;
		}, 0);

		await Bun.sleep(10);
		expect(calls).toBe(0);
		scheduler.setPaused(false);
		await Bun.sleep(10);
		expect(calls).toBe(1);
		scheduler.dispose();
	});
});
