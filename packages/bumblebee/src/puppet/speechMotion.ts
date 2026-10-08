import type { SpeechMotionFrame } from "../utils/speechMotion";

export const PUPPET_TALK_HZ = 4.35;

export const puppetTalkIntensityForFrame = (frame: SpeechMotionFrame) =>
	Math.min(1.4, frame.talkIntensity * 0.72);

export const speechOcclusionForFrame = (baseOcclusion: number, normalizedLevel: number) => {
	const intensity = Math.pow(Math.max(0, Math.min(1, normalizedLevel)), 0.72);
	return Math.max(baseOcclusion, Math.min(0.95, baseOcclusion + 0.04 * (1 - intensity)));
};

/** The live puppet's speech-driven rocking, also sampled by offline presentations. */
export const puppetSpeechRotation = (phase: number, intensity: number) => {
	const amplitude = 0.006 + 0.025 * intensity;
	return Math.sin(phase) * amplitude + Math.sin(phase * 2.1 + 0.7) * amplitude * 0.18;
};
