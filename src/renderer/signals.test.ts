import { afterEach, expect, test } from "bun:test";
import { createSignals } from "./signals";
import type { AudioMix, OverlayEvent } from "../lib/types";
const originalAudio = globalThis.Audio;
afterEach(() => {
  globalThis.Audio = originalAudio;
});
test("a stopped cue cannot restart when its pending play resolves; loops release both audio nodes", async () => {
  let resolve!: () => void,
    pauses = 0,
    disconnects = 0,
    volume = 1;
  class TestAudio {
    crossOrigin = "";
    src = "";
    loop = false;
    volume = 1;
    onended: null | (() => void) = null;
    onerror: null | (() => void) = null;
    pause() {
      pauses++;
    }
    play() {
      return new Promise<void>((done) => (resolve = done));
    }
  }
  globalThis.Audio = TestAudio as unknown as typeof Audio;
  const context = {
    state: "running",
    destination: {},
    createMediaElementSource() {
      return {
        connect() {},
        disconnect() {
          disconnects++;
        },
      };
    },
    createGain() {
      return {
        gain: {
          get value() {
            return volume;
          },
          set value(v: number) {
            volume = v;
          },
        },
        connect() {},
        disconnect() {
          disconnects++;
        },
      };
    },
  } as unknown as AudioContext;
  let mix: AudioMix = {
    output: "overlay",
    masterVolume: 1,
    bumblebeeTtsVolume: 1,
    puppetTtsVolume: 1,
    flyingSoundVolume: 0,
    wakeChirpVolume: 1,
    thinkingSoundVolume: 0.2,
    chatTtsWaitingToneVolume: 1,
  };
  const signals = createSignals(
    "http://127.0.0.1:2899/token/",
    () => context,
    () => mix,
    () => false,
  );
  const event: Extract<OverlayEvent, { type: "signal" }> = {
    type: "signal",
    id: "cue",
    kind: "thinking",
    audio_path: "tone.wav",
    gain: 0.2,
    looping: true,
    audible: true,
  };
  const pending = signals.start(event);
  expect(volume).toBe(0.2);
  mix = { ...mix, thinkingSoundVolume: 0.6 };
  signals.update();
  expect(volume).toBe(0.6);
  signals.stop(event.id);
  expect(disconnects).toBe(2);
  expect(pauses).toBe(1);
  resolve();
  await pending;
  expect(pauses).toBe(2);
  await signals.start(event);
  expect(disconnects).toBe(2);
  signals.dispose();
  expect(disconnects).toBe(2);
});
