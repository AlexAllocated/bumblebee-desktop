const BYTE_AUDIO_CENTER = 128;
const BYTE_AUDIO_MAX_DEVIATION = 128;
const DEFAULT_SMOOTHING_ALPHA = 0.2;

const calculateByteRmsLevel = (samples: ArrayLike<number>) => {
	if (samples.length === 0) return 0;

	let sumSq = 0;
	for (let index = 0; index < samples.length; index++) {
		const deviation = samples[index] - BYTE_AUDIO_CENTER;
		sumSq += deviation * deviation;
	}

	return Math.sqrt(sumSq / samples.length) / BYTE_AUDIO_MAX_DEVIATION;
};

const smoothAudioLevel = (
	previousLevel: number,
	nextLevel: number,
	alpha = DEFAULT_SMOOTHING_ALPHA
) => {
	const clampedAlpha = Math.max(0, Math.min(1, alpha));
	return clampedAlpha * nextLevel + (1 - clampedAlpha) * previousLevel;
};

export { calculateByteRmsLevel, smoothAudioLevel };
