import { createActor } from "xstate";
import { createRuntimeHost } from "../runtimeHost";
import { overlayMachine } from "./machines";
import { OverlayEventEmitter } from "./events";
import { AssetResolver } from "./assetResolver";
import { Surface } from "./surface";
import { Audio } from "./audio";
import { SpeechSession, type SpeechTargetActor } from "./speechSession";
import {
  ChatBubbles,
  type BubbleOptions,
  type BubblePresentationUpdate,
} from "./chatBubbles";
import { Nameplates, type NameplateOptions } from "./nameplates";
import {
  StreamerVoiceBubblePreview,
  type StreamerVoiceBubblePreviewOptions,
} from "./streamerVoiceBubblePreview";
import { BumblebeeActor } from "../actors/bumblebee";
import type { PuppetActor } from "../actors/puppet";
import { soundEffectKeys } from "./sounds";
import { attachPlaybackClock, PlaybackClock } from "./playbackClock";
import type {
  AdoptActorOptions,
  AdoptPuppetOptions,
  BubbleHandle,
  BubbleMessage,
  Bumblebee,
  BumblebeeOptions,
  ChatPuppetOverlayOptions,
  EventUnsubscribe,
  Overlay,
  OverlayEventFor,
  OverlayEventSubscriptionOptions,
  OverlayEventType,
  OverlayOptions,
  OverlaySurfaceEffect,
  OverlaySurfaceTarget,
  Puppet,
  PuppetOptions,
  Speech,
  SpeechTarget,
  SoundEffect,
  SoundEffectPlaybackOptions,
  VoiceChirpKind,
} from "./types";

type PuppetSpeechTarget = Extract<SpeechTarget, { type: "puppet" }>;

export const findPuppetActorByTarget = <T extends { actorId: string }>(
  actors: Iterable<T>,
  target: PuppetSpeechTarget,
) => {
  if (!target.actorId) return null;
  return [...actors].find((actor) => actor.actorId === target.actorId) ?? null;
};

const targetFor = (target: Speech["target"]): SpeechTarget => {
  const speechTarget = (target as { speechTarget?: SpeechTarget }).speechTarget;
  if (speechTarget) return speechTarget;
  const actorId = "actorId" in target ? target.actorId : undefined;
  if (typeof actorId === "string") {
    if (actorId === "bumblebee:default") return { type: "bumblebee" };
    if (actorId.startsWith("puppet:")) {
      return { type: "puppet", id: actorId.slice("puppet:".length) };
    }
    return { type: "actor", actorId };
  }
  return target as SpeechTarget;
};

const puppetCacheKey = (target: PuppetSpeechTarget) => {
  const instanceId = target.instanceId?.trim();
  return instanceId || target.id;
};

const normalizeChatPuppetOptions = (
  options?: ChatPuppetOverlayOptions,
): ChatPuppetOverlayOptions => ({
  position: options?.position ?? undefined,
  scale: typeof options?.scale === "number" ? options.scale : undefined,
  occlusion:
    typeof options?.occlusion === "number" ? options.occlusion : undefined,
});

const chatPuppetPositionTarget = (
  position: ChatPuppetOverlayOptions["position"],
) => (position ? { x: position.horizontalPercent / 100, y: 0.82 } : undefined);

const chatPuppetPose = (
  options: ChatPuppetOverlayOptions,
): ChatPuppetOverlayOptions => ({
  position: options.position ?? undefined,
  scale: options.scale,
  occlusion: options.occlusion,
});

const hasChatPuppetPose = (options: ChatPuppetOverlayOptions) =>
  Boolean(options.position) ||
  typeof options.scale === "number" ||
  typeof options.occlusion === "number";

export const createElementViewport =
  (container: HTMLElement): NonNullable<BubbleOptions["viewport"]> =>
  () => {
    const rect = container.getBoundingClientRect();
    return {
      left: 0,
      top: 0,
      width: rect.width,
      height: rect.height,
    };
  };

const createPuppetActor = async (
  runtime: OverlayRuntime,
  id: string,
  options?: PuppetOptions,
) => {
  const { PuppetActor } = await import("../actors/puppet");
  return new PuppetActor(runtime, id, options).init(options);
};

export const resolveChatPuppetSurfaceOptions = (
  options: Pick<
    OverlayOptions,
    "surface" | "zIndex" | "chatPuppetSurface" | "chatPuppetZIndex"
  >,
) => {
  if (!options.chatPuppetSurface && options.chatPuppetZIndex === undefined)
    return null;
  return {
    surface: options.chatPuppetSurface ?? options.surface,
    zIndex: options.chatPuppetZIndex ?? options.zIndex,
  };
};

export const resolveOverlayDecorationZIndexes = (
  options: Pick<OverlayOptions, "zIndex" | "chatPuppetZIndex">,
) => {
  const primarySurfaceZIndex = options.zIndex ?? 2147482990;
  const puppetSurfaceZIndex = options.chatPuppetZIndex ?? primarySurfaceZIndex;
  const foregroundSurfaceZIndex = Math.max(
    primarySurfaceZIndex,
    puppetSurfaceZIndex,
  );
  return {
    bubbles: foregroundSurfaceZIndex + 1,
    nameplates: foregroundSurfaceZIndex + 2,
  };
};

export const createSpeechPuppetOptions = (
  target: PuppetSpeechTarget,
  chatPuppets?: ChatPuppetOverlayOptions,
): PuppetOptions => {
  const cacheKey = puppetCacheKey(target);
  const defaults = normalizeChatPuppetOptions(chatPuppets);
  const position = chatPuppetPositionTarget(defaults.position);
  return {
    ...(cacheKey === target.id ? {} : { instanceId: cacheKey }),
    ...(target.imageUrl == null ? {} : { imageUrl: target.imageUrl }),
    ...(target.imageMask == null ? {} : { imageMask: target.imageMask }),
    ...(target.stickColor == null ? {} : { stickColor: target.stickColor }),
    visible: false,
    ...(position ? { position } : {}),
    ...(defaults.scale === undefined ? {} : { scale: defaults.scale }),
    ...(defaults.occlusion === undefined
      ? {}
      : { occlusion: defaults.occlusion }),
  };
};

const createManualBubbleId = () => {
  if (globalThis.crypto && "randomUUID" in globalThis.crypto) {
    return `manual:${globalThis.crypto.randomUUID()}`;
  }
  return `manual:${Date.now().toString(36)}:${Math.random().toString(36).slice(2)}`;
};

const createAdoptedPuppetKey = (
  puppetId: string,
  controller: AdoptPuppetOptions["controller"],
) => {
  const nodeId = (controller.node as { uniqueId?: number }).uniqueId;
  if (typeof nodeId === "number" && Number.isFinite(nodeId)) {
    return `${puppetId}:node:${nodeId}`;
  }
  if (globalThis.crypto && "randomUUID" in globalThis.crypto) {
    return `${puppetId}:adopted:${globalThis.crypto.randomUUID()}`;
  }
  return `${puppetId}:adopted:${Date.now().toString(36)}:${Math.random().toString(36).slice(2)}`;
};

const normalizeBubbleDuration = (durationMs: unknown) =>
  typeof durationMs === "number" && Number.isFinite(durationMs)
    ? Math.max(450, Math.min(15_000, durationMs))
    : 1800;

export class OverlayRuntime implements Overlay {
  readonly options: OverlayOptions;
  readonly actorRef = createActor(overlayMachine).start();
  readonly events = new OverlayEventEmitter();
  readonly host;
  readonly assets;
  readonly surface: Surface;
  readonly audio: Audio;
  readonly chatBubbles: ChatBubbles;
  readonly nameplates: Nameplates;
  readonly streamerVoiceBubblePreview: StreamerVoiceBubblePreview;
  readonly speechSession: SpeechSession;
  #puppetSurface: Surface | null = null;
  #bumblebee: BumblebeeActor | null = null;
  #puppets = new Map<string, PuppetActor>();
  #chatPuppetOptions: ChatPuppetOverlayOptions;
  #chatPuppetCacheKeys = new Set<string>();
  #adoptedPuppets = new Map<string, SpeechTargetActor>();
  #adoptedPuppetReleases = new Map<string, EventUnsubscribe>();
  #adoptedActors = new Map<string, SpeechTargetActor>();
  #adoptedActorReleases = new Map<string, EventUnsubscribe>();
  #manualBubbleCleanups = new Map<
    string,
    (options?: { immediate?: boolean }) => Promise<void>
  >();
  #disposePromise: Promise<void> | null = null;
  readonly #playbackClock = new PlaybackClock();

  constructor(options: OverlayOptions) {
    this.options = options;
    const decorationZIndexes = resolveOverlayDecorationZIndexes(this.options);
    this.actorRef.send({ type: "START" });
    this.host = createRuntimeHost(this.options);
    this.assets = new AssetResolver(this.host, this.options.assets);
    try {
      this.surface = new Surface({
        surface: this.options.surface,
        zIndex: this.options.zIndex,
      });
    } catch (error) {
      this.actorRef.stop();
      throw error;
    }

    attachPlaybackClock(this.surface.scene, this.#playbackClock);
    this.audio = new Audio();
    this.#chatPuppetOptions = normalizeChatPuppetOptions(
      this.options.chatPuppets,
    );
    this.chatBubbles = new ChatBubbles(
      typeof this.options.bubbles === "boolean"
        ? this.options.bubbles
        : {
            ...this.options.bubbles,
            zIndex: this.options.bubbles?.zIndex ?? decorationZIndexes.bubbles,
          },
      {
        onPageExit: (page) => {
          this.events.emit({
            type: "bubble:page-exit",
            audioId: page.sourceId,
            pageId: page.id,
            chunkIndex: page.chunkIndex,
            text: page.text,
          });
        },
      },
    );
    this.nameplates = new Nameplates(
      typeof this.options.nameplates === "boolean"
        ? this.options.nameplates
        : {
            ...this.options.nameplates,
            zIndex:
              this.options.nameplates?.zIndex ?? decorationZIndexes.nameplates,
          },
    );
    this.streamerVoiceBubblePreview = new StreamerVoiceBubblePreview({
      zIndex: (this.options.zIndex ?? 2147482990) + 12,
    });
    this.speechSession = new SpeechSession({
      audio: this.audio,
      chatBubbles: this.chatBubbles,
      events: this.events,
      prepareSpeech: this.options.prepareSpeech,
      actors: {
        resolveSpeechTarget: (target) => this.#resolveSpeechTarget(target),
        resolveDefaultBumblebee: () => this.#resolveDefaultBumblebee(),
      },
    });
    this.actorRef.send({ type: "READY" });
    this.events.emit({ type: "ready" });
  }

  getCanvas() {
    return this.surface.scene.getEngine().getRenderingCanvas();
  }

  get audioContext() {
    return this.audio.context;
  }

  get audioOutput() {
    return this.audio.output;
  }

  get puppetSurface() {
    const surfaceOptions = resolveChatPuppetSurfaceOptions(this.options);
    if (!surfaceOptions) return this.surface;
    if (!this.#puppetSurface) {
      this.#puppetSurface = new Surface(surfaceOptions);
      attachPlaybackClock(this.#puppetSurface.scene, this.#playbackClock);
      this.#puppetSurface.setPaused(this.#playbackClock.paused);
    }
    return this.#puppetSurface;
  }

  playbackNow() {
    return this.#playbackClock.now();
  }

  get playbackPaused() {
    return this.#playbackClock.paused;
  }

  setPlaybackTimeout(callback: () => void, delayMs: number) {
    return this.#playbackClock.setTimeout(callback, delayMs);
  }

  clearPlaybackTimeout(
    timer: ReturnType<PlaybackClock["setTimeout"]> | null | undefined,
  ) {
    this.#playbackClock.clearTimeout(timer);
  }

  puppetImageUrl(id: string) {
    return this.assets.puppetImage(id);
  }

  async bumblebee(options?: BumblebeeOptions): Promise<Bumblebee> {
    if (this.#disposePromise) throw new Error("Overlay has been disposed.");
    if (this.#bumblebee) {
      this.#bumblebee.configure(options);
      if (options?.visible) await this.#bumblebee.show();
      return this.#bumblebee;
    }
    const actor = await new BumblebeeActor(this).init(options);
    if (this.#disposePromise) {
      await actor.dispose();
      throw new Error("Overlay has been disposed.");
    }
    this.#bumblebee = actor;
    this.events.emit({
      type: "actor:created",
      actorId: actor.actorId,
      actorType: "bumblebee",
    });
    return actor;
  }

  async puppet(id: string, options?: PuppetOptions): Promise<Puppet> {
    if (this.#disposePromise) throw new Error("Overlay has been disposed.");
    const cacheKey = options?.instanceId?.trim() || id;
    const existing = this.#puppets.get(cacheKey);
    if (existing) {
      if (existing.id !== id || !existing.usesVisualSource(options)) {
        await existing.dispose();
      } else {
        await existing.configure(options);
        if (typeof options?.hideAfterSpeech === "boolean") {
          existing.setAutoHideAfterSpeech(options.hideAfterSpeech);
        } else if (options?.visible) {
          existing.setAutoHideAfterSpeech(false);
        } else if (
          options &&
          "visible" in options &&
          options.visible === false
        ) {
          existing.setAutoHideAfterSpeech(true);
        }
        if (options?.visible) await existing.show();
        return existing;
      }
    }
    const actor = await createPuppetActor(this, id, options);
    if (this.#disposePromise) {
      await actor.dispose();
      throw new Error("Overlay has been disposed.");
    }
    actor.setAutoHideAfterSpeech(
      options?.hideAfterSpeech ?? !(options?.visible ?? false),
    );
    this.#puppets.set(actor.cacheKey, actor);
    this.events.emit({
      type: "actor:created",
      actorId: actor.actorId,
      actorType: "puppet",
    });
    return actor;
  }

  async puppetActor(id: string, options?: PuppetOptions): Promise<PuppetActor> {
    return (await this.puppet(id, options)) as PuppetActor;
  }

  adoptActor(options: AdoptActorOptions): EventUnsubscribe {
    const actorId = options.actorId.trim();
    if (!actorId)
      throw new Error("Adopted actors require a non-empty actorId.");
    this.#adoptedActorReleases.get(actorId)?.();
    const actor: SpeechTargetActor = {
      actorId,
      speechTarget: { type: "actor", actorId },
      startTalking: () => {
        void options.onBeforeSpeak?.();
      },
      stopTalking: () => {
        void options.onAfterSpeak?.();
      },
      getChatBubbleScreenRect: options.getChatBubbleScreenRect,
      getChatBubbleViewport: options.getChatBubbleViewport,
    };
    this.#adoptedActors.set(actorId, actor);
    const release = () => {
      if (this.#adoptedActors.get(actorId) !== actor) return;
      this.#adoptedActors.delete(actorId);
      this.#adoptedActorReleases.delete(actorId);
    };
    this.#adoptedActorReleases.set(actorId, release);
    return release;
  }

  adoptPuppet(options: AdoptPuppetOptions): EventUnsubscribe {
    const adoptedKey = createAdoptedPuppetKey(
      options.puppetId,
      options.controller,
    );
    this.#adoptedPuppetReleases.get(adoptedKey)?.();
    const nameplateId = `adopted-puppet:${adoptedKey}`;
    const scene = options.controller.node.getScene();
    const baseNameplate = options.nameplate ?? null;
    const currentAnchor = () =>
      options.getNameplateAnchor?.() ??
      options.controller.getNameplateScreenAnchor?.({ stable: false }) ??
      null;
    const showNameplate = (
      nameplate: NonNullable<AdoptPuppetOptions["nameplate"]>,
    ) => {
      this.nameplates.show(nameplateId, nameplate, currentAnchor());
    };
    const restoreBaseNameplate = () => {
      if (baseNameplate) {
        showNameplate(baseNameplate);
      } else {
        this.nameplates.hide(nameplateId);
      }
    };
    const observer = scene.onAfterRenderObservable.add(() => {
      this.nameplates.updateGeometry(nameplateId, currentAnchor());
    });
    const actor: SpeechTargetActor = {
      actorId: `adopted-puppet:${adoptedKey}`,
      speechTarget: {
        type: "puppet",
        id: options.puppetId,
        instanceId: adoptedKey,
      },
      show: async () => undefined,
      startTalking: () => {
        void options.onBeforeSpeak?.();
        void options.controller.talk();
      },
      stopTalking: () => {
        void options.controller.shutup();
        void options.onAfterSpeak?.();
        restoreBaseNameplate();
      },
      updateSpeechMotion: (frame) => {
        options.controller.setTalkingSpeed?.(frame.talkSpeed);
        options.controller.setTalkingIntensity?.(frame.talkIntensity);
      },
      getChatBubbleScreenRect: () =>
        options.getChatBubbleScreenRect?.() ??
        options.controller.getChatDecorationScreenRect?.({ stable: true }) ??
        null,
      getChatBubbleViewport: options.getChatBubbleViewport,
      getNameplateAnchor: () =>
        options.getNameplateAnchor?.() ??
        options.controller.getNameplateScreenAnchor?.({ stable: false }) ??
        null,
      setNameplate: (nameplate) => {
        if (!nameplate) {
          restoreBaseNameplate();
          return;
        }
        this.nameplates.show(nameplateId, nameplate, currentAnchor());
      },
      refreshNameplate: () => {
        this.nameplates.updateGeometry(nameplateId, currentAnchor());
      },
    };
    if (baseNameplate) {
      actor.setNameplate?.(baseNameplate);
    }
    this.#adoptedPuppets.set(adoptedKey, actor);
    if (!this.#adoptedPuppets.has(options.puppetId)) {
      this.#adoptedPuppets.set(options.puppetId, actor);
    }
    this.events.emit({
      type: "actor:created",
      actorId: actor.actorId,
      actorType: "puppet",
    });
    const release = () => {
      if (this.#adoptedPuppets.get(adoptedKey) !== actor) return;
      scene.onAfterRenderObservable.remove(observer);
      this.nameplates.remove(nameplateId);
      this.#adoptedPuppets.delete(adoptedKey);
      this.#adoptedPuppetReleases.delete(adoptedKey);
      if (this.#adoptedPuppets.get(options.puppetId) === actor) {
        this.#adoptedPuppets.delete(options.puppetId);
      }
      this.events.emit({
        type: "actor:disposed",
        actorId: actor.actorId,
        actorType: "puppet",
      });
    };
    this.#adoptedPuppetReleases.set(adoptedKey, release);
    return release;
  }

  async configureChatPuppets(
    update?: ChatPuppetOverlayOptions,
    options?: { animate?: boolean; durationMs?: number },
  ) {
    this.#chatPuppetOptions = normalizeChatPuppetOptions(update);
    if (!hasChatPuppetPose(this.#chatPuppetOptions)) return;
    const pose = chatPuppetPose(this.#chatPuppetOptions);
    await Promise.all(
      [...this.#chatPuppetCacheKeys].map(async (cacheKey) => {
        const actor = this.#puppets.get(cacheKey);
        if (!actor) {
          this.#chatPuppetCacheKeys.delete(cacheKey);
          return;
        }
        await actor.setPose(pose, options);
      }),
    );
  }

  setSurfaceContainer(container?: HTMLElement | null) {
    return this.surface.setContainer(container);
  }

  setChatPuppetSurfaceContainer(container?: HTMLElement | null) {
    if (!resolveChatPuppetSurfaceOptions(this.options)) {
      throw new Error(
        "Chat puppet surface retargeting requires a dedicated chat puppet surface or z-index.",
      );
    }
    return this.puppetSurface.setContainer(container);
  }

  setChatPuppetDecorationContainer(container?: HTMLElement | null) {
    const target = container ?? null;
    this.chatBubbles.updateOptions({
      container: target,
      viewport: target ? createElementViewport(target) : undefined,
    });
    this.nameplates.setContainer(target);
  }

  setSurfaceEffect(target: OverlaySurfaceTarget, effect: OverlaySurfaceEffect) {
    const surface =
      target === "chat-puppets" ? this.puppetSurface : this.surface;
    surface.setEffect(effect);
  }

  showStreamerVoiceBubblePreview(options: StreamerVoiceBubblePreviewOptions) {
    this.streamerVoiceBubblePreview.show(options);
  }

  refreshStreamerVoiceBubblePreview(
    options: StreamerVoiceBubblePreviewOptions,
  ) {
    this.streamerVoiceBubblePreview.refresh(options);
  }

  async hideStreamerVoiceBubblePreview(options?: { immediate?: boolean }) {
    await this.streamerVoiceBubblePreview.hide(options);
  }

  async showBubble(message: BubbleMessage): Promise<BubbleHandle | null> {
    const text = message.text.trim();
    if (!text || !this.chatBubbles.enabled) return null;
    const target = targetFor(message.target);
    const actor = await this.#resolveSpeechTarget(target);
    if (!actor?.getChatBubbleScreenRect) return null;
    const geometry = actor.getChatBubbleScreenRect();
    if (!geometry) return null;
    const id = createManualBubbleId();
    const subject = target.type === "puppet" ? "puppet" : "bumblebee";
    let hidden = false;
    let frame: number | null = null;
    let trackTimer: ReturnType<typeof setTimeout> | null = null;
    let hideTimer: ReturnType<PlaybackClock["setTimeout"]> | null = null;
    const stopTracking = () => {
      if (frame !== null && typeof cancelAnimationFrame === "function") {
        cancelAnimationFrame(frame);
      }
      frame = null;
      if (trackTimer) clearTimeout(trackTimer);
      trackTimer = null;
    };
    const scheduleGeometryUpdate = () => {
      if (hidden) return;
      const update = () => {
        frame = null;
        trackTimer = null;
        if (hidden) return;
        if (this.#playbackClock.paused) {
          scheduleGeometryUpdate();
          return;
        }
        this.chatBubbles.updateGeometry(
          id,
          actor.getChatBubbleScreenRect?.() ?? null,
        );
        scheduleGeometryUpdate();
      };
      if (typeof requestAnimationFrame === "function") {
        frame = requestAnimationFrame(update);
      } else {
        trackTimer = setTimeout(update, 16);
      }
    };
    const hide = async (options?: { immediate?: boolean }) => {
      if (hidden) return;
      hidden = true;
      this.#playbackClock.clearTimeout(hideTimer);
      hideTimer = null;
      stopTracking();
      this.#manualBubbleCleanups.delete(id);
      await this.chatBubbles.end(id, options);
      this.events.emit({
        type: "bubble:end",
        audioId: id,
        target: actor.speechTarget,
      });
    };

    this.#manualBubbleCleanups.set(id, hide);
    this.chatBubbles.start(
      {
        audioId: id,
        text,
        style: message.style ?? null,
        subject,
        puppetId: target.type === "puppet" ? target.id : null,
        actorId: actor.actorId,
      },
      geometry,
      {
        autoReveal: message.manualReveal ? false : (message.autoReveal ?? true),
        manualReveal: message.manualReveal,
      },
    );
    this.events.emit({
      type: "bubble:start",
      audioId: id,
      target: actor.speechTarget,
    });
    scheduleGeometryUpdate();
    hideTimer = this.#playbackClock.setTimeout(
      () => void hide(),
      normalizeBubbleDuration(message.durationMs),
    );
    return {
      id,
      reveal: (revealedText, options) => {
        if (hidden) return;
        this.chatBubbles.reveal(
          id,
          revealedText,
          options?.chunkIndex ?? 0,
          options?.fullCursor,
        );
      },
      hide,
    };
  }

  getBubblePage(audioId: string) {
    return this.chatBubbles.getPage(audioId);
  }

  updateBubble(audioId: string, options: BubblePresentationUpdate) {
    this.chatBubbles.updatePresentation(audioId, options);
  }

  async say(speech: Speech) {
    const target = targetFor(speech.target);
    await this.speechSession.say(speech, target);
  }

  interruptSpeech(reason?: string) {
    this.speechSession?.interruptSpeech(reason);
  }

  refreshActorBubbleGeometry(actor: SpeechTargetActor) {
    this.speechSession?.refreshActorBubbleGeometry(actor);
  }

  async playSound(
    effect: SoundEffect,
    volume = 1,
    options?: SoundEffectPlaybackOptions,
  ) {
    return this.audio.playEffect(
      effect,
      this.assets.soundEffect(effect),
      volume,
      options,
    );
  }

  async playCallWaitingChime(volume = 1) {
    return this.audio.playCallWaitingChime(volume);
  }

  async playVoiceChirp(kind: VoiceChirpKind, volume = 1) {
    return this.audio.playVoiceChirp(kind, volume);
  }

  async playThinkingSound(durationMs = 1000, volume = 1) {
    return this.audio.playThinkingSound(durationMs, volume);
  }

  async preloadSounds(effects: SoundEffect[] = soundEffectKeys) {
    await Promise.all(
      effects.map((effect) =>
        this.audio.preloadEffect(effect, this.assets.soundEffect(effect)),
      ),
    );
  }

  setVolume(value: number) {
    this.audio.setVolume(value);
  }

  setAutomaticAudioResumeEnabled(enabled: boolean) {
    this.audio.setAutomaticResumeEnabled(enabled);
  }

  async setPlaybackPaused(paused: boolean) {
    if (paused === this.#playbackClock.paused) return;
    this.#playbackClock.setPaused(paused);
    this.surface.setPaused(paused);
    this.#puppetSurface?.setPaused(paused);
    this.chatBubbles.setPaused(paused);
    this.nameplates.setPaused(paused);
    this.audio.setAutomaticResumeEnabled(!paused);
    if (paused) {
      if (this.audio.context.state === "running")
        await this.audio.context.suspend();
      return;
    }
    if (this.audio.context.state === "suspended") await this.audio.resume();
  }

  setBubbleOptions(options: BubbleOptions | boolean) {
    this.chatBubbles.updateOptions(
      typeof options === "boolean"
        ? options
        : {
            ...options,
            zIndex: options.zIndex ?? (this.options.zIndex ?? 2147482990) + 1,
          },
    );
  }

  setNameplateOptions(options: NameplateOptions | boolean) {
    this.nameplates.updateOptions(
      typeof options === "boolean"
        ? options
        : {
            ...options,
            zIndex: options.zIndex ?? (this.options.zIndex ?? 2147482990) + 2,
          },
    );
  }

  on<T extends OverlayEventType>(
    event: T,
    handler: (event: OverlayEventFor<T>) => void,
    options?: OverlayEventSubscriptionOptions,
  ): EventUnsubscribe {
    return this.events.on(event, handler, options);
  }

  dispose() {
    this.#disposePromise ??= this.#disposeResources();
    return this.#disposePromise;
  }

  async #disposeResources() {
    this.actorRef.send({ type: "DISPOSE" });
    const manualBubbleCleanups = [...this.#manualBubbleCleanups.values()];
    await Promise.all(
      manualBubbleCleanups.map((cleanup) =>
        cleanup({ immediate: true }).catch(() => undefined),
      ),
    );
    this.#manualBubbleCleanups.clear();
    await this.speechSession?.dispose();
    for (const actor of [...this.#puppets.values()]) {
      await actor.dispose();
    }
    this.#puppets.clear();
    await this.streamerVoiceBubblePreview.dispose();
    for (const release of [...this.#adoptedPuppetReleases.values()]) {
      release();
    }
    this.#adoptedPuppetReleases.clear();
    this.#adoptedPuppets.clear();
    for (const release of [...this.#adoptedActorReleases.values()]) {
      release();
    }
    this.#adoptedActorReleases.clear();
    this.#adoptedActors.clear();
    if (this.#bumblebee) {
      await this.#bumblebee.dispose();
      this.#bumblebee = null;
    }
    this.chatBubbles.dispose();
    this.nameplates.dispose();
    await this.audio.dispose();
    this.#playbackClock.dispose();
    this.#puppetSurface?.dispose();
    this.#puppetSurface = null;
    this.surface.dispose();
    this.events.emit({ type: "runtime:disposed" });
    this.events.clear();
    this.actorRef.send({ type: "DISPOSED" });
    this.actorRef.stop();
  }

  forgetBumblebee(actor: BumblebeeActor) {
    if (this.#bumblebee === actor) this.#bumblebee = null;
  }

  forgetPuppet(actor: PuppetActor) {
    if (this.#puppets.get(actor.cacheKey) === actor)
      this.#puppets.delete(actor.cacheKey);
    this.#chatPuppetCacheKeys.delete(actor.cacheKey);
  }

  async #resolveSpeechTarget(
    target: SpeechTarget,
  ): Promise<SpeechTargetActor | null> {
    if (target.type === "bumblebee") return this.#resolveDefaultBumblebee();
    if (target.type === "puppet") {
      const actor = findPuppetActorByTarget(this.#puppets.values(), target);
      if (actor) return actor;
      return this.#resolvePuppet(target);
    }
    if (target.actorId === this.#bumblebee?.actorId) return this.#bumblebee;
    return (
      this.#adoptedActors.get(target.actorId) ??
      [...this.#puppets.values()].find(
        (actor) => actor.actorId === target.actorId,
      ) ??
      null
    );
  }

  async #resolvePuppet(
    targetOrId: PuppetSpeechTarget | string,
  ): Promise<SpeechTargetActor | null> {
    const target: PuppetSpeechTarget =
      typeof targetOrId === "string"
        ? { type: "puppet", id: targetOrId }
        : targetOrId;
    const cacheKey = puppetCacheKey(target);
    const adopted =
      this.#adoptedPuppets.get(cacheKey) ?? this.#adoptedPuppets.get(target.id);
    if (adopted) return adopted;
    const options = createSpeechPuppetOptions(target, this.#chatPuppetOptions);
    const existing = this.#puppets.get(cacheKey);
    if (existing?.usesVisualSource(options)) {
      this.#puppets.delete(cacheKey);
      this.#puppets.set(cacheKey, existing);
      this.#chatPuppetCacheKeys.add(cacheKey);
      if (hasChatPuppetPose(this.#chatPuppetOptions)) {
        await existing.setPose(chatPuppetPose(this.#chatPuppetOptions), {
          animate: false,
          durationMs: 1,
        });
      }
      return existing;
    }
    if (existing) await existing.dispose();
    if (this.#puppets.size >= 28) {
      const idle = [...this.#puppets.values()].find(
        (actor) => !this.speechSession.isActorActive(actor.actorId),
      );
      if (idle) {
        this.#chatPuppetCacheKeys.delete(idle.cacheKey);
        await idle.dispose();
      }
    }
    const actor = await createPuppetActor(this, target.id, options);
    if (this.#disposePromise) {
      await actor.dispose();
      throw new Error("Overlay has been disposed.");
    }
    actor.setAutoHideAfterSpeech(target.hideAfterSpeech ?? true);
    this.#puppets.set(actor.cacheKey, actor);
    this.#chatPuppetCacheKeys.add(actor.cacheKey);
    this.events.emit({
      type: "actor:created",
      actorId: actor.actorId,
      actorType: "puppet",
    });
    return actor;
  }

  async #resolveDefaultBumblebee(): Promise<SpeechTargetActor | null> {
    return this.#bumblebee ?? null;
  }
}

export const createOverlay = async (
  options: OverlayOptions,
): Promise<Overlay> => new OverlayRuntime(options);
