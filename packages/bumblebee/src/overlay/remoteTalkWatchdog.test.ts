import { describe, expect, test } from "bun:test";
import {
	REMOTE_TALK_MAX_WATCHDOG_MS,
	REMOTE_TALK_MIN_WATCHDOG_MS,
	REMOTE_TALK_WATCHDOG_PADDING_MS,
	resolveRemoteTalkWatchdogMs
} from "./remoteTalkWatchdog";

describe("remote talk watchdog policy", () => {
	test("uses explicit audio duration with padding and a minimum grace window", () => {
		expect(resolveRemoteTalkWatchdogMs({ audioDurationMs: 1_000 })).toBe(
			REMOTE_TALK_MIN_WATCHDOG_MS
		);
		expect(resolveRemoteTalkWatchdogMs({ audioDurationMs: 35_000 })).toBe(
			35_000 + REMOTE_TALK_WATCHDOG_PADDING_MS
		);
	});

	test("falls back to timeline duration and caps stale sessions", () => {
		expect(
			resolveRemoteTalkWatchdogMs({
				audioDurationMs: null,
				timeline: {
					words: [
						{ offsetMs: 1_000, durationMs: 3_000, text: "hello" },
						{ offsetMs: 116_000, durationMs: 10_000, text: "there" }
					],
					visemes: [{ offsetMs: 2_000, visemeId: 1 }]
				}
			})
		).toBe(REMOTE_TALK_MAX_WATCHDOG_MS);
	});

	test("uses the max watchdog when no reliable duration is available", () => {
		expect(resolveRemoteTalkWatchdogMs({})).toBe(REMOTE_TALK_MAX_WATCHDOG_MS);
		expect(resolveRemoteTalkWatchdogMs({ audioDurationMs: Number.NaN })).toBe(
			REMOTE_TALK_MAX_WATCHDOG_MS
		);
	});
});
