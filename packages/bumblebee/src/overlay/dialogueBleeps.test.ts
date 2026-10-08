import { describe, expect, test } from "bun:test";
import {
	DIALOGUE_BLEEP_DISPLAY_TEXT,
	DIALOGUE_BLEEP_PLACEHOLDER,
	prepareDialogueBleeps,
	renderDialogueBleeps,
	resolveDialogueBleepCues
} from "./dialogueBleeps";

describe("dialogue bleep placeholders", () => {
	test("prepares repeated placeholders without changing timeline offsets after each replacement", () => {
		const prepared = prepareDialogueBleeps(
			`${DIALOGUE_BLEEP_PLACEHOLDER} this ${DIALOGUE_BLEEP_PLACEHOLDER} thing`
		);

		expect(prepared).toEqual({
			spokenText: "bleep this bleep thing",
			displayText: `${DIALOGUE_BLEEP_DISPLAY_TEXT} this ${DIALOGUE_BLEEP_DISPLAY_TEXT} thing`,
			spokenTextOffsets: [0, 11]
		});
		expect(prepared.spokenText.length).toBe(prepared.displayText.length);
	});

	test("renders the authoring placeholder as comic censor symbols", () => {
		expect(renderDialogueBleeps(`What the ${DIALOGUE_BLEEP_PLACEHOLDER}?`)).toBe(
			`What the ${DIALOGUE_BLEEP_DISPLAY_TEXT}?`
		);
	});

	test("maps placeholder offsets to padded speech-timeline cues", () => {
		expect(
			resolveDialogueBleepCues([0, 11], {
				words: [
					{ offsetMs: 100, durationMs: 240, text: "bleep", textOffset: 0, wordLength: 5 },
					{ offsetMs: 390, durationMs: 100, text: "this", textOffset: 6, wordLength: 4 },
					{ offsetMs: 540, durationMs: 280, text: "bleep", textOffset: 11, wordLength: 5 }
				]
			})
		).toEqual([
			{ offsetMs: 85, durationMs: 285 },
			{ offsetMs: 525, durationMs: 475 }
		]);
	});

	test("falls back to ordered bleep words when timeline text offsets are absent", () => {
		expect(
			resolveDialogueBleepCues([0], {
				words: [{ offsetMs: 25, durationMs: 80, text: "bleep" }]
			})
		).toEqual([{ offsetMs: 10, durationMs: 275 }]);
	});

	test("holds each censor through the pause before the next spoken word", () => {
		expect(
			resolveDialogueBleepCues([0, 6], {
				words: [
					{ offsetMs: 80, durationMs: 90, text: "bleep", textOffset: 0, wordLength: 5 },
					{ offsetMs: 360, durationMs: 75, text: "bleep", textOffset: 6, wordLength: 5 },
					{ offsetMs: 650, durationMs: 100, text: "it", textOffset: 12, wordLength: 2 }
				]
			})
		).toEqual([
			{ offsetMs: 65, durationMs: 275 },
			{ offsetMs: 345, durationMs: 285 }
		]);
	});
});
