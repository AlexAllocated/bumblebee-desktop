type SpeechMotionFrame = {
	active: boolean;
	level: number;
	normalizedLevel: number;
	mouthOpen: number;
	talkSpeed: number;
	talkIntensity: number;
	peakPulse: number;
};

type SpeechMotionOptions = {
	noiseFloor?: number;
	minAdaptivePeak?: number;
	peakDecayPerSecond?: number;
	attackPerSecond?: number;
	releasePerSecond?: number;
	mouthAttackPerSecond?: number;
	mouthReleasePerSecond?: number;
	silenceHoldMs?: number;
	peakThreshold?: number;
	peakRefractoryMs?: number;
	speedMin?: number;
	speedMax?: number;
	speedCurve?: number;
};

const defaultOptions = {
	noiseFloor: 0.004,
	minAdaptivePeak: 0.032,
	peakDecayPerSecond: 0.78,
	attackPerSecond: 18,
	releasePerSecond: 7,
	mouthAttackPerSecond: 24,
	mouthReleasePerSecond: 10,
	silenceHoldMs: 135,
	peakThreshold: 0.38,
	peakRefractoryMs: 95,
	speedMin: 0.62,
	speedMax: 1.02,
	speedCurve: 0.92
};

const visemeMouthOpen = new Map<number, number>([
	[0, 0],
	[1, 0.95],
	[2, 1],
	[3, 0.82],
	[4, 0.68],
	[5, 0.48],
	[6, 0.34],
	[7, 0.36],
	[8, 0.58],
	[9, 0.82],
	[10, 0.66],
	[11, 0.84],
	[12, 0.24],
	[13, 0.28],
	[14, 0.32],
	[15, 0.16],
	[16, 0.24],
	[17, 0.2],
	[18, 0.18],
	[19, 0.14],
	[20, 0.12],
	[21, 0.06]
]);

const clamp01 = (value: number) => Math.max(0, Math.min(1, value));

const approach = (current: number, target: number, perSecond: number, dtSeconds: number) => {
	const alpha = 1 - Math.exp(-perSecond * dtSeconds);
	return current + (target - current) * alpha;
};

const normalizeLevel = (level: number, peak: number, noiseFloor: number) => {
	const span = Math.max(0.001, peak - noiseFloor);
	return clamp01((level - noiseFloor) / span);
};

class SpeechMotion {
	#options = defaultOptions;
	#adaptivePeak = defaultOptions.minAdaptivePeak;
	#smoothedLevel = 0;
	#mouthOpen = 0;
	#lastUpdateMs: number | null = null;
	#lastVoiceMs = -Infinity;
	#lastPeakMs = -Infinity;
	#previousPeakSource = 0;

	constructor(options?: SpeechMotionOptions) {
		this.#options = { ...defaultOptions, ...options };
		this.reset();
	}

	reset(_nowMs = 0) {
		this.#adaptivePeak = this.#options.minAdaptivePeak;
		this.#smoothedLevel = 0;
		this.#mouthOpen = 0;
		this.#lastUpdateMs = null;
		this.#lastVoiceMs = -Infinity;
		this.#lastPeakMs = -Infinity;
		this.#previousPeakSource = 0;
	}

	stop(nowMs = this.#lastUpdateMs ?? 0): SpeechMotionFrame {
		this.reset(nowMs);
		return this.#frame(0, 0, 0, false);
	}

	updateLevel(level: number, nowMs: number): SpeechMotionFrame {
		const rawLevel = clamp01(Number.isFinite(level) ? level : 0);
		return this.#update(rawLevel, nowMs);
	}

	updateViseme(visemeId: number, nowMs: number): SpeechMotionFrame {
		return this.#update(clamp01(visemeMouthOpen.get(visemeId) ?? 0.2), nowMs, {
			alreadyNormalized: true
		});
	}

	#update(
		level: number,
		nowMs: number,
		options?: {
			alreadyNormalized?: boolean;
		}
	): SpeechMotionFrame {
		const previousUpdateMs = this.#lastUpdateMs;
		const firstUpdate = previousUpdateMs === null;
		const dtSeconds = firstUpdate
			? 1 / 60
			: Math.max(1 / 120, Math.min(0.12, (nowMs - previousUpdateMs) / 1000));
		this.#lastUpdateMs = nowMs;
		let peakSource = 0;

		if (options?.alreadyNormalized) {
			peakSource = level;
			this.#smoothedLevel = firstUpdate
				? level
				: approach(
						this.#smoothedLevel,
						level,
						level > this.#smoothedLevel
							? this.#options.attackPerSecond
							: this.#options.releasePerSecond,
						dtSeconds
					);
		} else {
			this.#adaptivePeak = Math.max(
				this.#options.minAdaptivePeak,
				this.#adaptivePeak * Math.pow(this.#options.peakDecayPerSecond, dtSeconds),
				level
			);
			const normalizedTarget = normalizeLevel(level, this.#adaptivePeak, this.#options.noiseFloor);
			peakSource = normalizedTarget;
			this.#smoothedLevel = firstUpdate
				? normalizedTarget
				: approach(
						this.#smoothedLevel,
						normalizedTarget,
						normalizedTarget > this.#smoothedLevel
							? this.#options.attackPerSecond
							: this.#options.releasePerSecond,
						dtSeconds
					);
		}

		const normalized = clamp01(this.#smoothedLevel);
		const voiceDetected = options?.alreadyNormalized
			? normalized > 0.06
			: level > this.#options.noiseFloor;
		if (voiceDetected) {
			this.#lastVoiceMs = nowMs;
		}
		const active = voiceDetected || nowMs - this.#lastVoiceMs < this.#options.silenceHoldMs;
		const peakPulse =
			peakSource >= this.#options.peakThreshold &&
			this.#previousPeakSource < this.#options.peakThreshold &&
			nowMs - this.#lastPeakMs >= this.#options.peakRefractoryMs
				? 1
				: 0;
		if (peakPulse) {
			this.#lastPeakMs = nowMs;
		}
		this.#previousPeakSource = peakSource;

		const mouthTarget = active ? clamp01(Math.pow(normalized, 0.62) + peakPulse * 0.16) : 0;
		this.#mouthOpen = approach(
			this.#mouthOpen,
			mouthTarget,
			mouthTarget > this.#mouthOpen
				? this.#options.mouthAttackPerSecond
				: this.#options.mouthReleasePerSecond,
			dtSeconds
		);

		return this.#frame(level, normalized, peakPulse, active);
	}

	#frame(
		level: number,
		normalizedLevel: number,
		peakPulse: number,
		active: boolean
	): SpeechMotionFrame {
		const talkSpeed =
			this.#options.speedMin +
			(this.#options.speedMax - this.#options.speedMin) *
				Math.pow(normalizedLevel, this.#options.speedCurve);
		return {
			active,
			level,
			normalizedLevel,
			mouthOpen: active ? clamp01(this.#mouthOpen) : 0,
			talkSpeed,
			talkIntensity: active ? clamp01(0.18 + normalizedLevel * 0.82 + peakPulse * 0.16) : 0,
			peakPulse
		};
	}
}

export { SpeechMotion, visemeMouthOpen };
export type { SpeechMotionFrame };
