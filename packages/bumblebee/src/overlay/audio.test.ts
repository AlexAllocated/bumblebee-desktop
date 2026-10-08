import { describe, expect, test } from "bun:test";
import {
	CALL_WAITING_CHIME,
	THINKING_SOUND_LEVEL,
	THINKING_SOUND_LOOP_MS,
	THINKING_SOUND_VARIANTS,
	VOICE_CHIRPS,
	createReversedAudioBuffer,
	resolveSpeechAudioFilter
} from "./audio";

describe("CALL_WAITING_CHIME", () => {
	test("matches the shared Songbird courtesy cue", () => {
		expect(CALL_WAITING_CHIME).toEqual({
			level: 0.26,
			notes: [
				{ frequencyHz: 523, durationMs: 70, fadeInMs: 6, fadeOutMs: 18, delayMs: 0 },
				{ frequencyHz: 659, durationMs: 85, fadeInMs: 6, fadeOutMs: 18, delayMs: 115 }
			]
		});
	});
});

describe("VOICE_CHIRPS", () => {
	test("matches the shared Songbird wake and heard cues", () => {
		expect(VOICE_CHIRPS).toEqual({
			wake_chirp: {
				level: 0.26,
				notes: [
					{ startHz: 620, endHz: 980, durationMs: 70, fadeInMs: 4, fadeOutMs: 12, delayMs: 0 },
					{ startHz: 700, endHz: 1160, durationMs: 85, fadeInMs: 4, fadeOutMs: 12, delayMs: 98 }
				]
			},
			heard_chirp: {
				level: 0.22,
				notes: [
					{ startHz: 1120, endHz: 760, durationMs: 55, fadeInMs: 5, fadeOutMs: 16, delayMs: 0 },
					{ startHz: 960, endHz: 620, durationMs: 72, fadeInMs: 5, fadeOutMs: 16, delayMs: 79 }
				]
			}
		});
	});
});

describe("THINKING_SOUND_VARIANTS", () => {
	test("matches the production thinking-loop shape", () => {
		expect(THINKING_SOUND_LOOP_MS).toBe(1000);
		expect(THINKING_SOUND_LEVEL).toBe(0.1);
		expect(THINKING_SOUND_VARIANTS).toHaveLength(3);
		for (const variant of THINKING_SOUND_VARIANTS) {
			expect(variant).toHaveLength(6);
		}
	});
});

describe("resolveSpeechAudioFilter", () => {
	test("normalizes a consumer-microphone profile to safe Web Audio values", () => {
		expect(
			resolveSpeechAudioFilter(
				{
					highpassHz: 180,
					lowpassHz: 5200,
					presence: { frequencyHz: 1350, gainDb: 3.5, q: 0.9 },
					compressor: {
						thresholdDb: -30,
						kneeDb: 16,
						ratio: 4.5,
						attackSeconds: 0.008,
						releaseSeconds: 0.18
					}
				},
				48_000
			)
		).toEqual({
			highpassHz: 180,
			lowpassHz: 5200,
			presence: { frequencyHz: 1350, gainDb: 3.5, q: 0.9 },
			compressor: {
				thresholdDb: -30,
				kneeDb: 16,
				ratio: 4.5,
				attackSeconds: 0.008,
				releaseSeconds: 0.18
			}
		});
	});
});

describe("createReversedAudioBuffer", () => {
	test("reverses every channel without changing the source buffer", () => {
		const sourceChannels = [new Float32Array([0.1, 0.2, 0.3]), new Float32Array([-0.4, 0, 0.4])];
		const targetChannels = [new Float32Array(3), new Float32Array(3)];
		const source = {
			numberOfChannels: 2,
			length: 3,
			sampleRate: 48_000,
			getChannelData: (channel: number) => sourceChannels[channel]
		} as AudioBuffer;
		const reversed = {
			getChannelData: (channel: number) => targetChannels[channel]
		} as AudioBuffer;
		const context = {
			createBuffer: () => reversed
		} as unknown as BaseAudioContext;

		expect(createReversedAudioBuffer(context, source)).toBe(reversed);
		expect(Array.from(targetChannels[0])).toEqual([
			expect.closeTo(0.3),
			expect.closeTo(0.2),
			expect.closeTo(0.1)
		]);
		expect(Array.from(targetChannels[1])).toEqual([expect.closeTo(0.4), 0, expect.closeTo(-0.4)]);
		expect(Array.from(sourceChannels[0])).toEqual([
			expect.closeTo(0.1),
			expect.closeTo(0.2),
			expect.closeTo(0.3)
		]);
	});
});
