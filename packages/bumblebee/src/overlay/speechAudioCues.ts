import type { SpeechAudioCue } from "./types";
import type { SpeechTimeline } from "./chatBubbles";

export type TimedSpeechAudioCue = {
	kind: SpeechAudioCue["kind"];
	offsetMs: number;
	volume: number;
};

const normalizeWord = (value: string) =>
	value
		.toLocaleLowerCase()
		.replace(/[^\p{L}\p{N}']/gu, "")
		.trim();

export const resolveSpeechAudioCues = (
	cues: readonly SpeechAudioCue[],
	timeline: SpeechTimeline | null | undefined
): TimedSpeechAudioCue[] => {
	if (!cues.length || !timeline?.words?.length) return [];
	const timelineWords = timeline.words.map((word) => normalizeWord(word.text));
	const resolved: TimedSpeechAudioCue[] = [];

	for (const cue of cues) {
		const phrase = cue.afterText.split(/\s+/u).map(normalizeWord).filter(Boolean);
		if (!phrase.length) continue;
		let matchEndIndex = -1;
		for (let start = 0; start <= timelineWords.length - phrase.length; start += 1) {
			if (phrase.every((word, index) => timelineWords[start + index] === word)) {
				matchEndIndex = start + phrase.length - 1;
			}
		}
		const matchedWord = timeline.words[matchEndIndex];
		if (!matchedWord) continue;
		const offsetMs = Math.max(0, matchedWord.offsetMs + matchedWord.durationMs + 10);
		resolved.push({
			kind: cue.kind,
			offsetMs,
			volume: Math.max(0, Math.min(1, cue.volume ?? 1))
		});
	}

	return resolved;
};
