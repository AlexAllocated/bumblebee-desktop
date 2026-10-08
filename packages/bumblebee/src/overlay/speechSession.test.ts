import { afterEach, describe, expect, mock, test } from "bun:test";
import {
  SpeechSession,
  speechCursorAt,
  type SpeechTargetActor,
} from "./speechSession";
import { OverlayEventEmitter } from "./events";
import type { Audio } from "./audio";
import type { ChatBubbles } from "./chatBubbles";
import type { Speech } from "./types";

const originalFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = originalFetch;
});
const deferred = <T>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
};
const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
const target = { type: "bumblebee" } as const;
const speech = (id: string): Speech => ({
  id,
  target,
  text: "Hello again!",
  audio: {
    url: `https://local.invalid/${id}`,
    words: [
      { text: "Hello", startMs: 100, durationMs: 100 },
      { text: "again", startMs: 500, durationMs: 100 },
    ],
  },
});
function harness() {
  globalThis.fetch = mock(
    async () => new Response(new Uint8Array([1, 2, 3])),
  ) as unknown as typeof fetch;
  const order: string[] = [];
  const plays: NonNullable<Parameters<Audio["play"]>[1]>[] = [];
  const completions: Array<() => void> = [];
  const actor: SpeechTargetActor = {
    actorId: "bee",
    speechTarget: target,
    show: mock(async () => {
      order.push("show");
    }),
    startTalking: mock(() => {
      order.push("talk");
    }),
    stopTalking: mock(() => {}),
    settleSpeechMotion: mock(async () => {}),
  };
  const audio = {
    play: mock(
      async (
        _bytes: Uint8Array,
        options: NonNullable<Parameters<Audio["play"]>[1]>,
      ) => {
        plays.push(options);
        await options.onReady?.(1000);
        if (options.signal?.aborted) return false;
        options.onStart?.(1000);
        order.push("play");
        await new Promise<void>((resolve) => {
          completions.push(resolve);
          options.signal?.addEventListener("abort", () => resolve(), {
            once: true,
          });
        });
        return !options.signal?.aborted;
      },
    ),
  } as unknown as Audio;
  const bubbles = {
    start: mock(() => {
      order.push("bubble");
    }),
    reveal: mock(() => {}),
    end: mock(async () => {}),
    updateGeometry: mock(() => {}),
  };
  const events = new OverlayEventEmitter();
  events.on("speech:start", () => {
    order.push("speech:start");
  });
  events.on("speech:end", () => {
    order.push("speech:end");
  });
  const options = {
    audio,
    chatBubbles: bubbles as unknown as ChatBubbles,
    events,
    actors: {
      resolveDefaultBumblebee: mock(async () => actor),
      resolveSpeechTarget: mock(async () => actor),
    },
  };
  const session = new SpeechSession(options);
  return {
    session,
    options,
    actor,
    audio,
    bubbles,
    events,
    plays,
    completions,
    order,
  };
}

describe("prepared speech lifecycle", () => {
  test("package owns queue, native clock, and event order before playback", async () => {
    const h = harness();
    const first = h.session.say(speech("one"), target);
    const second = h.session.say(speech("two"), target);
    await tick();
    expect(h.plays).toHaveLength(1);
    expect(h.order.slice(0, 5)).toEqual([
      "show",
      "speech:start",
      "bubble",
      "talk",
      "play",
    ]);
    h.plays[0].onTime?.(0);
    expect(h.bubbles.reveal).not.toHaveBeenCalled();
    h.plays[0].onTime?.(100);
    expect(h.bubbles.reveal).toHaveBeenLastCalledWith("one", "Hello ", 0, 6);
    h.plays[0].onTime?.(100);
    expect(h.bubbles.reveal).toHaveBeenCalledTimes(1);
    h.plays[0].onTime?.(500);
    expect(h.bubbles.reveal).toHaveBeenLastCalledWith(
      "one",
      "Hello again!",
      0,
      12,
    );
    h.completions[0]();
    await first;
    await tick();
    expect(h.plays).toHaveLength(2);
    h.completions[1]();
    await second;
    await h.session.dispose();
  });
  test("interrupt clears active and queued speech without automatic replay", async () => {
    const h = harness();
    const promises = [
      h.session.say(speech("one"), target),
      h.session.say(speech("two"), target),
    ];
    await tick();
    h.session.interruptSpeech();
    await Promise.all(promises);
    await tick();
    expect(h.plays).toHaveLength(1);
    expect(h.plays[0].signal?.aborted).toBe(true);
    expect(h.bubbles.end).toHaveBeenCalledWith("one", { immediate: true });
    const next = h.session.say(speech("new"), target);
    await tick();
    expect(h.plays).toHaveLength(2);
    h.completions[1]();
    await next;
    await h.session.dispose();
  });
  test("abandoned preparation cannot block the next turn or play when it finally returns", async () => {
    const h = harness();
    const waiting = deferred<{ url: string }>();
    const prepare = mock(() => waiting.promise);
    const session = new SpeechSession({ ...h.options, prepareSpeech: prepare });
    const old = session.say({ target, text: "Unprepared" }, target);
    session.interruptSpeech();
    await old;
    const next = session.say(speech("new"), target);
    await tick();
    expect(h.plays).toHaveLength(1);
    waiting.resolve({ url: "https://local.invalid/old" });
    await tick();
    expect(globalThis.fetch).toHaveBeenCalledTimes(1);
    h.completions[0]();
    await next;
    await session.dispose();
  });
  test("late actor resolution after interruption never starts playback", async () => {
    const h = harness();
    const waiting = deferred<SpeechTargetActor>();
    h.options.actors.resolveDefaultBumblebee.mockImplementation(
      () => waiting.promise,
    );
    const pending = h.session.say(speech("old"), target);
    await tick();
    h.session.interruptSpeech();
    await pending;
    await h.session.dispose();
    waiting.resolve(h.actor);
    await tick();
    expect(h.audio.play).not.toHaveBeenCalled();
    expect(h.actor.show).not.toHaveBeenCalled();
  });
  test("bounds pending speech and settles all canceled jobs", async () => {
    const h = harness();
    const jobs = Array.from({ length: 9 }, (_, index) =>
      h.session.say(speech(String(index)), target),
    );
    await expect(h.session.say(speech("overflow"), target)).rejects.toThrow(
      "queue is full",
    );
    h.session.interruptSpeech();
    await Promise.all(jobs);
    await h.session.dispose();
  });
  test("a caller may enqueue a turn from completion without stranding the queue", async () => {
    const h = harness();
    const follow = h.session
      .say(speech("first"), target)
      .then(() => h.session.say(speech("follow"), target));
    await tick();
    h.completions[0]();
    await tick();
    expect(h.plays).toHaveLength(2);
    h.completions[1]();
    await follow;
    await h.session.dispose();
  });
  test("hidden subjects retain audio while suppressing actor and bubble visuals", async () => {
    const h = harness();
    const pending = h.session.say(
      {
        ...speech("hidden"),
        visuals: false,
        bubbles: false,
        nameplates: false,
      },
      target,
    );
    await tick();
    expect(h.actor.show).not.toHaveBeenCalled();
    expect(h.actor.startTalking).not.toHaveBeenCalled();
    expect(h.bubbles.start).not.toHaveBeenCalled();
    h.completions[0]();
    await pending;
    await h.session.dispose();
  });
  test("word mapping ignores SSML offsets and respects repeated text and punctuation", () => {
    const timeline = {
      words: [
        { text: "Hello", offsetMs: 100, durationMs: 20, textOffset: 102 },
        { text: "hello", offsetMs: 200, durationMs: 20, textOffset: 120 },
      ],
    };
    expect(speechCursorAt("Hello, hello!", timeline, 99)).toBe(0);
    expect(speechCursorAt("Hello, hello!", timeline, 100)).toBe(7);
    expect(speechCursorAt("Hello, hello!", timeline, 200)).toBe(13);
  });
});
