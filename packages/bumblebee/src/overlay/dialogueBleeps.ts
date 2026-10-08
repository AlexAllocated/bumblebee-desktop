import type { SpeechTimeline } from "@hivetech/speech-bubbles";

export const DIALOGUE_BLEEP_PLACEHOLDER = "[BLEEP]";
export const DIALOGUE_BLEEP_DISPLAY_TEXT = "$#@%!";
const DIALOGUE_BLEEP_SPOKEN_TEXT = "bleep";

export type PreparedDialogueBleeps = {
	spokenText: string;
	displayText: string;
	spokenTextOffsets: number[];
};

export type DialogueBleepCue = {
	offsetMs: number;
	durationMs: number;
};

export const prepareDialogueBleeps = (text: string): PreparedDialogueBleeps => {
	let cursor = 0;
	let marker = text.indexOf(DIALOGUE_BLEEP_PLACEHOLDER);
	let spokenText = "";
	let displayText = "";
	const spokenTextOffsets: number[] = [];

	while (marker >= 0) {
		const leadingText = text.slice(cursor, marker);
		spokenText += leadingText;
		displayText += leadingText;
		spokenTextOffsets.push(spokenText.length);
		spokenText += DIALOGUE_BLEEP_SPOKEN_TEXT;
		displayText += DIALOGUE_BLEEP_DISPLAY_TEXT;
		cursor = marker + DIALOGUE_BLEEP_PLACEHOLDER.length;
		marker = text.indexOf(DIALOGUE_BLEEP_PLACEHOLDER, cursor);
	}

	spokenText += text.slice(cursor);
	displayText += text.slice(cursor);
	return { spokenText, displayText, spokenTextOffsets };
};

export const renderDialogueBleeps = (text: string) => prepareDialogueBleeps(text).displayText;

const isFiniteNonNegative = (value: unknown): value is number =>
	typeof value === "number" && Number.isFinite(value) && value >= 0;

export const resolveDialogueBleepCues = (
	spokenTextOffsets: readonly number[],
	timeline: SpeechTimeline | null | undefined
): DialogueBleepCue[] => {
	if (!spokenTextOffsets.length || !timeline?.words?.length) return [];
	const unusedWords = new Set(timeline.words.map((_, index) => index));
	const matchedWordIndexes: number[] = [];

	for (const textOffset of spokenTextOffsets) {
		let wordIndex = timeline.words.findIndex(
			(word, index) =>
				unusedWords.has(index) &&
				word.text.toLowerCase() === DIALOGUE_BLEEP_SPOKEN_TEXT &&
				word.textOffset === textOffset
		);
		if (wordIndex < 0) {
			wordIndex = timeline.words.reduce((bestIndex, word, index) => {
				if (!unusedWords.has(index) || word.text.toLowerCase() !== DIALOGUE_BLEEP_SPOKEN_TEXT) {
					return bestIndex;
				}
				if (bestIndex < 0) return index;
				const bestOffset = timeline.words?.[bestIndex]?.textOffset;
				if (!isFiniteNonNegative(word.textOffset)) return bestIndex;
				if (!isFiniteNonNegative(bestOffset)) return index;
				return Math.abs(word.textOffset - textOffset) < Math.abs(bestOffset - textOffset)
					? index
					: bestIndex;
			}, -1);
		}
		const word = timeline.words[wordIndex];
		if (!word || !isFiniteNonNegative(word.offsetMs)) continue;
		unusedWords.delete(wordIndex);
		matchedWordIndexes.push(wordIndex);
	}

	return matchedWordIndexes.map((wordIndex) => {
		const word = timeline.words?.[wordIndex];
		if (!word || !isFiniteNonNegative(word.offsetMs)) {
			return { offsetMs: 0, durationMs: 0 };
		}
		const wordDurationMs = isFiniteNonNegative(word.durationMs) ? word.durationMs : 0;
		const leadMs = Math.min(15, word.offsetMs);
		const cueStartMs = word.offsetMs - leadMs;
		const nextWord = timeline.words?.[wordIndex + 1];
		const cueEndMs = isFiniteNonNegative(nextWord?.offsetMs)
			? Math.max(word.offsetMs + wordDurationMs, nextWord.offsetMs - 20)
			: word.offsetMs + wordDurationMs + 180;
		return {
			offsetMs: cueStartMs,
			durationMs: Math.max(190, cueEndMs - cueStartMs)
		};
	});
};
