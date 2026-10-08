import { describe, expect, mock, test } from "bun:test";
import { Audio } from "./audio";

const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
function install() {
  const keys = [
    "AudioContext",
    "window",
    "document",
    "requestAnimationFrame",
    "cancelAnimationFrame",
  ] as const;
  const previous = new Map(
    keys.map((key) => [key, Object.getOwnPropertyDescriptor(globalThis, key)]),
  );
  const frames = new Map<number, FrameRequestCallback>();
  let frameId = 0;
  const node = () => ({
    connect: mock(() => {}),
    disconnect: mock(() => {}),
    gain: {
      value: 1,
      setValueAtTime: mock(() => {}),
      setTargetAtTime: mock(() => {}),
    },
  });
  const createSource = () => {
    const source = {
      ...node(),
      buffer: null as AudioBuffer | null,
      onended: null as (() => void) | null,
      start: mock((_time: number) => {}),
      stop: mock(() => {
        source.onended?.();
      }),
    };
    return source;
  };
  let resolveDecode!: (buffer: AudioBuffer) => void;
  class Context {
    state = "running";
    currentTime = 10;
    sampleRate = 48000;
    destination = {};
    sources: Array<ReturnType<typeof createSource>> = [];
    decodeAudioData = mock(
      () =>
        new Promise<AudioBuffer>((resolve) => {
          resolveDecode = resolve;
        }),
    );
    createGain = node;
    createAnalyser = () => ({
      ...node(),
      fftSize: 2048,
      frequencyBinCount: 2,
      getByteTimeDomainData: (bytes: Uint8Array) => bytes.fill(128),
    });
    createBufferSource = () => {
      const source = createSource();
      this.sources.push(source);
      return source;
    };
    addEventListener() {}
    removeEventListener() {}
    close() {
      this.state = "closed";
      return Promise.resolve();
    }
  }
  Object.defineProperty(globalThis, "AudioContext", {
    value: Context,
    configurable: true,
  });
  Object.defineProperty(globalThis, "window", {
    value: { addEventListener() {}, removeEventListener() {} },
    configurable: true,
  });
  Object.defineProperty(globalThis, "document", {
    value: { addEventListener() {}, removeEventListener() {} },
    configurable: true,
  });
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    value: (callback: FrameRequestCallback) => {
      frames.set(++frameId, callback);
      return frameId;
    },
    configurable: true,
  });
  Object.defineProperty(globalThis, "cancelAnimationFrame", {
    value: (id: number) => frames.delete(id),
    configurable: true,
  });
  const audio = new Audio();
  const context = audio.context as unknown as Context;
  return {
    audio,
    context,
    decode: () => resolveDecode({ duration: 2 } as AudioBuffer),
    frame: (time: number) => {
      const callbacks = [...frames.values()];
      frames.clear();
      for (const callback of callbacks) callback(time);
    },
    restore: async () => {
      await audio.dispose();
      for (const key of keys) {
        const descriptor = previous.get(key);
        if (descriptor) Object.defineProperty(globalThis, key, descriptor);
        else Reflect.deleteProperty(globalThis, key);
      }
    },
  };
}
describe("WebAudio native speech clock", () => {
  test("caption time follows AudioContext and stays fixed while its clock is suspended", async () => {
    const h = install();
    try {
      const times: number[] = [];
      const pending = h.audio.play(new Uint8Array([1]), {
        onTime: (time) => times.push(time),
      });
      await tick();
      h.decode();
      await tick();
      const start = h.context.sources[0].start.mock.calls[0][0];
      h.context.currentTime = start + 0.25;
      h.frame(1000);
      h.frame(9000);
      expect(times).toEqual([250, 250]);
      h.context.currentTime = start + 0.75;
      h.frame(9050);
      expect(times.at(-1)).toBe(750);
      h.context.sources[0].onended?.();
      await pending;
    } finally {
      await h.restore();
    }
  });
  test("interrupt settles a pending decode and late decoded data cannot play", async () => {
    const h = install();
    try {
      const abort = new AbortController();
      const ready = mock(() => {});
      const pending = h.audio.play(new Uint8Array([1]), {
        signal: abort.signal,
        onReady: ready,
      });
      await tick();
      abort.abort();
      await expect(pending).rejects.toHaveProperty("name", "AbortError");
      h.decode();
      await tick();
      expect(h.context.sources).toHaveLength(0);
      expect(ready).not.toHaveBeenCalled();
    } finally {
      await h.restore();
    }
  });
});
