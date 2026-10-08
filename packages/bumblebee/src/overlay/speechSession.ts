import type { OverlayOptions, Speech, SpeechTarget } from "./types";
import type { Audio } from "./audio";
import type { OverlayEventEmitter } from "./events";
import type {
  BubbleScreenRect,
  ChatBubbles,
  SpeechTimeline,
} from "./chatBubbles";
import type { Viewport } from "@hivetech/speech-bubbles";
import type { NameplateAnchor, NameplatePayload } from "./nameplates";
import { SpeechMotion, type SpeechMotionFrame } from "../utils/speechMotion";
import { normalizeChatBubbleStyle } from "./chatBubbleStyles";
import { abortable } from "../utils/abortable";
import { speechTimelineCursorAt } from "@hivetech/speech-bubbles/reveal";
import { resolveSpeechAudioCues } from "./speechAudioCues";

export type SpeechTargetActor = {
  readonly actorId: string;
  readonly speechTarget: SpeechTarget;
  show?: () => Promise<void>;
  hide?: () => Promise<void>;
  startTalking: () => void;
  stopTalking: () => void;
  updateSpeechMotion?: (frame: SpeechMotionFrame) => void;
  settleSpeechMotion?: () => Promise<void> | void;
  shouldHideAfterSpeech?: () => boolean;
  getChatBubbleScreenRect?: () => BubbleScreenRect | null;
  getChatBubbleViewport?: () => Viewport;
  getNameplateAnchor?: () => NameplateAnchor | null;
  setNameplate?: (nameplate: NameplatePayload | null) => void;
  refreshNameplate?: () => void;
};
type Job = {
  speech: Speech;
  target: SpeechTarget;
  abort: AbortController;
  resolve: () => void;
  reject: (reason: unknown) => void;
};
type Options = {
  audio: Audio;
  chatBubbles: ChatBubbles;
  events: OverlayEventEmitter;
  prepareSpeech?: OverlayOptions["prepareSpeech"];
  actors: {
    resolveSpeechTarget: (
      target: SpeechTarget,
    ) => Promise<SpeechTargetActor | null>;
    resolveDefaultBumblebee: () => Promise<SpeechTargetActor | null>;
  };
};

/** Word boundaries use the provider's exact offsets and the decoded audio playback clock. */
export const speechCursorAt = speechTimelineCursorAt;

/** Rendering-only speech orchestration. No provider clients, credentials or hosted bot sessions. */
export class SpeechSession {
  #queue: Job[] = [];
  #current: Job | null = null;
  #actor: SpeechTargetActor | null = null;
  #audioId: string | null = null;
  #drain: Promise<void> | null = null;
  #disposed = false;
  constructor(readonly options: Options) {}
  isActorActive(actorId: string) {
    return this.#actor?.actorId === actorId;
  }
  say(speech: Speech, target: SpeechTarget): Promise<void> {
    if (this.#disposed)
      return Promise.reject(new Error("Overlay has been disposed."));
    if (this.#queue.length >= 8)
      return Promise.reject(new Error("Overlay speech queue is full."));
    const result = new Promise<void>((resolve, reject) =>
      this.#queue.push({
        speech,
        target,
        abort: new AbortController(),
        resolve,
        reject,
      }),
    );
    this.#kick();
    return result;
  }
  #kick() {
    if (!this.#drain && !this.#disposed)
      this.#drain = this.#run().finally(() => {
        this.#drain = null;
        if (this.#queue.length) this.#kick();
      });
  }
  async #run() {
    while (!this.#disposed && this.#queue.length) {
      const job = this.#queue.shift()!;
      this.#current = job;
      try {
        await this.#play(job);
        job.resolve();
      } catch (reason) {
        if (job.abort.signal.aborted) job.resolve();
        else job.reject(reason);
      } finally {
        this.#current = null;
        this.#actor = null;
        this.#audioId = null;
      }
    }
  }
  async #play(job: Job) {
    const { speech, target } = job;
    const signal = job.abort.signal;
    const prepared =
      speech.audio ??
      (speech.file
        ? { url: speech.file }
        : await abortable(
            Promise.resolve(this.options.prepareSpeech?.(speech, signal)),
            signal,
          ));
    signal.throwIfAborted();
    if (!prepared)
      throw new Error(
        "Provide prepared speech audio or an application prepareSpeech callback.",
      );
    const response = await fetch(prepared.url, {
      signal: AbortSignal.any([signal, AbortSignal.timeout(30_000)]),
    });
    if (!response.ok)
      throw new Error(`Speech audio could not load (HTTP ${response.status}).`);
    const bytes = new Uint8Array(
      await abortable(response.arrayBuffer(), signal),
    );
    signal.throwIfAborted();
    if (bytes.length > 32 * 1024 * 1024)
      throw new Error("Speech audio exceeds the 32 MB playback limit.");
    const actor = await abortable(
      target.type === "bumblebee"
        ? this.options.actors.resolveDefaultBumblebee()
        : this.options.actors.resolveSpeechTarget(target),
      signal,
    );
    signal.throwIfAborted();
    this.#actor = actor;
    const audioId = speech.id ?? crypto.randomUUID();
    this.#audioId = audioId;
    const text = speech.speechBubbleText ?? speech.text ?? "";
    const timeline: SpeechTimeline | null =
      speech.speechTimeline ??
      (prepared.words
        ? {
            words: prepared.words.map((word) => ({
              text: word.text,
              offsetMs: word.startMs,
              durationMs: word.durationMs,
            })),
          }
        : null);
    let lastCursor = -1,
      started = false;
    const motion = new SpeechMotion();
    try {
      await this.options.audio.play(bytes, {
        signal,
        bitcrusher: speech.audioBitcrusher,
        filter: speech.audioFilter,
        cues: resolveSpeechAudioCues(speech.audioCues ?? [], timeline),
        onReady: async (durationMs) => {
          signal.throwIfAborted();
          if (speech.visuals !== false) await actor?.show?.();
          signal.throwIfAborted();
          if (speech.nameplates !== false && speech.nameplateText)
            actor?.setNameplate?.({
              text: speech.nameplateText,
              platform: speech.platform,
              style: speech.nameplateStyle,
            });
          started = true;
          this.options.events.emit({ type: "speech:start", audioId, target });
          if (speech.bubbles !== false) {
            this.options.chatBubbles.start(
              {
                audioId,
                text,
                timeline,
                style: normalizeChatBubbleStyle(speech.chatBubbleStyle),
                subject: target.type === "bumblebee" ? "bumblebee" : "puppet",
                viewport:
                  speech.bubbleViewport ?? actor?.getChatBubbleViewport?.(),
                exitAnimation: speech.bubbleExitAnimation,
              },
              actor?.getChatBubbleScreenRect?.() ?? null,
              {
                autoReveal: false,
                manualReveal: true,
                audioDurationMs: durationMs,
              },
            );
            this.options.events.emit({ type: "bubble:start", audioId, target });
          }
        },
        onStart: () => {
          if (speech.visuals !== false) actor?.startTalking();
        },
        onLevel: (level, nowMs) => {
          if (!signal.aborted && speech.visuals !== false)
            actor?.updateSpeechMotion?.(motion.updateLevel(level, nowMs));
        },
        onTime: (elapsedMs) => {
          if (signal.aborted) return;
          if (speech.bubbles !== false) {
            const cursor = timeline?.words?.length
              ? speechCursorAt(text, timeline, elapsedMs)
              : text.length;
            if (cursor > 0 && cursor !== lastCursor) {
              this.options.chatBubbles.reveal(
                audioId,
                text.slice(0, cursor),
                0,
                cursor,
              );
              lastCursor = cursor;
            }
          }
          this.refreshActorBubbleGeometry(actor);
        },
      });
    } finally {
      try {
        actor?.stopTalking();
        await Promise.allSettled([
          Promise.resolve(actor?.settleSpeechMotion?.()),
          this.options.chatBubbles.end(audioId, { immediate: signal.aborted }),
          speech.visuals !== false && actor?.shouldHideAfterSpeech?.()
            ? Promise.resolve(actor.hide?.())
            : Promise.resolve(),
        ]);
      } finally {
        if (started) {
          this.options.events.emit({ type: "speech:end", audioId, target });
          if (speech.bubbles !== false)
            this.options.events.emit({ type: "bubble:end", audioId, target });
        }
      }
    }
  }
  refreshActorBubbleGeometry(actor: SpeechTargetActor | null) {
    if (actor && actor === this.#actor && this.#audioId) {
      this.options.chatBubbles.updateGeometry(
        this.#audioId,
        actor.getChatBubbleScreenRect?.() ?? null,
      );
      actor.refreshNameplate?.();
    }
  }
  interruptSpeech(_reason?: string) {
    this.#current?.abort.abort();
    this.#current?.resolve();
    for (const job of this.#queue.splice(0)) {
      job.abort.abort();
      job.resolve();
    }
    this.#actor?.stopTalking();
    if (this.#audioId)
      void this.options.chatBubbles.end(this.#audioId, { immediate: true });
  }
  async dispose() {
    this.#disposed = true;
    this.interruptSpeech("dispose");
    await this.#drain;
  }
}
