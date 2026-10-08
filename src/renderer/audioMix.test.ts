import { expect, test } from "bun:test";
import { speechGain, signalGain } from "./audioMix";
import type { AudioMix, Chatter, OverlayEvent } from "../lib/types";
const speech: Extract<OverlayEvent, { type: "speech" }> = {
  type: "speech",
  id: "clip",
  chatter: null,
  text: "hello",
  audio_path: "clip.wav",
  words: [],
  audible: true,
  gain: 0.25,
};
const mix: AudioMix = {
  output: "overlay",
  masterVolume: 0.5,
  bumblebeeTtsVolume: 0.5,
  puppetTtsVolume: 1.5,
  flyingSoundVolume: 0,
  wakeChirpVolume: 0.5,
  thinkingSoundVolume: 0.2,
  chatTtsWaitingToneVolume: 0.8,
};
test("gain received with speech is already mixed, and a newer live mix replaces it", () => {
  expect(speechGain(speech, null, false)).toBe(0.25);
  expect(speechGain(speech, mix, false)).toBe(0.25);
  expect(speechGain(speech, { ...mix, masterVolume: 1 }, false)).toBe(0.5);
  expect(speechGain({ ...speech, chatter: {} as Chatter }, mix, false)).toBe(
    0.75,
  );
});
test("Discord routing and local mute silence playback while retaining the media clock", () => {
  expect(speechGain(speech, { ...mix, output: "discord" }, false)).toBe(0);
  expect(speechGain({ ...speech, audible: false }, null, false)).toBe(0);
  expect(speechGain(speech, mix, true)).toBe(0);
  expect(speechGain({ ...speech, gain: 100 }, null, false)).toBe(2);
  expect(speechGain({ ...speech, gain: NaN }, null, false)).toBe(0);
});
test("active cue gains follow their own live settings and stop doubling the master gain", () => {
  const cue: Extract<OverlayEvent, { type: "signal" }> = {
    type: "signal",
    id: "thinking",
    audio_path: "thinking.wav",
    kind: "thinking",
    looping: true,
    audible: true,
    gain: 0.1,
  };
  expect(signalGain(cue, null, false)).toBe(0.1);
  expect(signalGain(cue, mix, false)).toBe(0.1);
  expect(signalGain(cue, { ...mix, thinkingSoundVolume: 0.8 }, false)).toBe(
    0.4,
  );
  expect(signalGain({ ...cue, kind: "waiting" }, mix, false)).toBe(0.4);
  expect(signalGain(cue, { ...mix, output: "discord" }, false)).toBe(0);
});

test("wake and heard keep their intrinsic ducking when a live settings event replaces their mixed gain", () => {
  const gains = { wake: 0.05, heard: 0.08, timeout: 0.25, cancel: 0.25 };
  for (const [kind, gain] of Object.entries(gains)) {
    const cue: Extract<OverlayEvent, { type: "signal" }> = {
      type: "signal",
      id: kind,
      audio_path: `${kind}.wav`,
      kind: kind as keyof typeof gains,
      looping: false,
      audible: true,
      gain,
    };
    // Initial event gain is already mixed; receiving the same live settings
    // must not make the cue louder or apply its ducking coefficient twice.
    expect(signalGain(cue, null, false)).toBeCloseTo(gain);
    expect(signalGain(cue, mix, false)).toBeCloseTo(gain);
    expect(signalGain(cue, { ...mix, wakeChirpVolume: 1 }, false)).toBeCloseTo(
      gain * 2,
    );
    expect(signalGain(cue, { ...mix, output: "discord" }, false)).toBe(0);
    expect(signalGain(cue, mix, true)).toBe(0);
  }
});
