import type { SpeechTimeline } from "./chatBubbles";

const REMOTE_TALK_MIN_WATCHDOG_MS = 15_000;
const REMOTE_TALK_MAX_WATCHDOG_MS = 120_000;
const REMOTE_TALK_WATCHDOG_PADDING_MS = 8_000;
const REMOTE_TALK_SILENCE_GRACE_MS = 5_000;

const readTimelineDurationMs = (timeline: SpeechTimeline | null | undefined) => {
	if (!timeline) return null;
	const wordEndMs = timeline.words?.reduce(
		(max, word) => Math.max(max, word.offsetMs + word.durationMs),
		0
	);
	const visemeEndMs = timeline.visemes?.reduce((max, viseme) => Math.max(max, viseme.offsetMs), 0);
	const durationMs = Math.max(wordEndMs ?? 0, visemeEndMs ?? 0);
	return durationMs > 0 ? durationMs : null;
};

const resolveRemoteTalkWatchdogMs = (input: {
	audioDurationMs?: number | null;
	timeline?: SpeechTimeline | null;
}) => {
	const durationMs =
		typeof input.audioDurationMs === "number" &&
		Number.isFinite(input.audioDurationMs) &&
		input.audioDurationMs > 0
			? input.audioDurationMs
			: readTimelineDurationMs(input.timeline);
	if (durationMs === null) return REMOTE_TALK_MAX_WATCHDOG_MS;
	return Math.max(
		REMOTE_TALK_MIN_WATCHDOG_MS,
		Math.min(REMOTE_TALK_MAX_WATCHDOG_MS, durationMs + REMOTE_TALK_WATCHDOG_PADDING_MS)
	);
};

export {
	REMOTE_TALK_MAX_WATCHDOG_MS,
	REMOTE_TALK_MIN_WATCHDOG_MS,
	REMOTE_TALK_SILENCE_GRACE_MS,
	REMOTE_TALK_WATCHDOG_PADDING_MS,
	resolveRemoteTalkWatchdogMs
};
