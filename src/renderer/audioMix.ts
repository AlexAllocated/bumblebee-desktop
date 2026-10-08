import type { AudioMix, OverlayEvent } from "../lib/types";

type Speech = Extract<OverlayEvent, { type: "speech" }>;
/** A newly received mix supersedes the gain captured when a queued clip was synthesized. */
export function speechGain(
  speech: Speech,
  mix: AudioMix | null,
  muted: boolean,
) {
  if (muted || (mix ? mix.output !== "overlay" : !speech.audible)) return 0;
  const gain = mix
    ? mix.masterVolume *
      (speech.chatter ? mix.puppetTtsVolume : mix.bumblebeeTtsVolume)
    : speech.gain;
  return Number.isFinite(gain) ? Math.max(0, Math.min(2, gain)) : 0;
}

type Signal = Extract<OverlayEvent, { type: "signal" }>;
export function signalGain(
  signal: Signal,
  mix: AudioMix | null,
  muted: boolean,
) {
  if (muted || (mix ? mix.output !== "overlay" : !signal.audible)) return 0;
  const gain = mix
    ? mix.masterVolume *
      (signal.kind === "thinking"
        ? mix.thinkingSoundVolume
        : signal.kind === "waiting"
          ? mix.chatTtsWaitingToneVolume
          : mix.wakeChirpVolume *
            (signal.kind === "wake" ? 0.2 : signal.kind === "heard" ? 0.32 : 1))
    : signal.gain;
  return Number.isFinite(gain) ? Math.max(0, Math.min(2, gain)) : 0;
}
