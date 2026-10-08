import { describe, expect, test } from "bun:test";
import { resolveSpeechAudioCues } from "./speechAudioCues";

describe("resolveSpeechAudioCues", () => {
	test("places a courtesy chime after the requested spoken phrase", () => {
		expect(
			resolveSpeechAudioCues(
				[{ kind: "call_waiting", afterText: "puts out updates all the", volume: 0.55 }],
				{
					words: [
						{ text: "puts", offsetMs: 1000, durationMs: 180 },
						{ text: "out", offsetMs: 1200, durationMs: 120 },
						{ text: "updates", offsetMs: 1350, durationMs: 300 },
						{ text: "all", offsetMs: 1680, durationMs: 140 },
						{ text: "the...", offsetMs: 1850, durationMs: 150 },
						{ text: "time.", offsetMs: 2350, durationMs: 180 }
					],
					visemes: []
				}
			)
		).toEqual([{ kind: "call_waiting", offsetMs: 2010, volume: 0.55 }]);
	});

	test("ignores a cue when its anchor phrase is absent", () => {
		expect(
			resolveSpeechAudioCues([{ kind: "call_waiting", afterText: "missing phrase" }], {
				words: [{ text: "Hello", offsetMs: 0, durationMs: 200 }],
				visemes: []
			})
		).toEqual([]);
	});

	test("preserves a wake chirp anchored after the wake phrase", () => {
		expect(
			resolveSpeechAudioCues([{ kind: "wake_chirp", afterText: "Hey Bumblebee", volume: 0.2 }], {
				words: [
					{ text: "Hey", offsetMs: 0, durationMs: 120 },
					{ text: "Bumblebee", offsetMs: 140, durationMs: 300 },
					{ text: "what's", offsetMs: 600, durationMs: 150 }
				],
				visemes: []
			})
		).toEqual([{ kind: "wake_chirp", offsetMs: 450, volume: 0.2 }]);
	});
});
