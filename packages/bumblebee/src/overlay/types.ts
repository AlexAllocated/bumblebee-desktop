import type { AnyActorRef } from "xstate";
import type { Viewport } from "@hivetech/speech-bubbles";
import type { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import type { Vector2, Vector3 } from "@babylonjs/core/Maths/math.vector";
import type {
  BubbleOptions,
  BubblePresentationUpdate,
  BubbleScreenRect,
  ChatBubblePage,
  SpeechTimeline,
} from "./chatBubbles";
import type { StreamerVoiceBubblePreviewOptions } from "./streamerVoiceBubblePreview";
import type { ChatBubbleStyleTokens, NameplateStyleTokens } from "../types";
import type { PuppetController as PuppetModelController } from "../puppet";
import type {
  NameplateAnchor,
  NameplateOptions,
  NameplatePlatform,
} from "./nameplates";
import type { ToolPresentationPayload } from "./toolPresentation";
import type { BabylonCanvasResizeOptions } from "../utils/babylonPerformance";
import type {
  OverlayMediaPlayPayload,
  OverlayMediaStartPayload,
  OverlayMediaStopPayload,
} from "./media";

export type { NameplatePlatform };

export type OverlayOptions = {
  assetBaseUrl?: string | URL;
  prepareSpeech?: (
    speech: Speech,
    signal: AbortSignal,
  ) => Promise<PreparedSpeechAudio>;
  assets?: OverlayAssetOptions;
  surface?: OverlaySurfaceOptions;
  chatPuppetSurface?: OverlaySurfaceOptions;
  chatPuppetZIndex?: number;
  zIndex?: number;
  editMode?: boolean;
  bubbles?: boolean | BubbleOptions;
  nameplates?: boolean | NameplateOptions;
  chatPuppets?: ChatPuppetOverlayOptions;
};

export type OverlayAssetOptions = {
  puppetImageUrl?: (id: string) => string;
  soundEffectUrls?: Partial<Record<SoundEffect, string | string[]>>;
  bumblebeeModelUrl?: string | URL;
};

export type OverlaySurfaceOptions = BabylonCanvasResizeOptions & {
  container?: HTMLElement | null;
};

export type OverlaySurfaceTarget = "bumblebee" | "chat-puppets";
export type OverlaySurfaceEffect = "phosphor" | "phosphor-red" | null;

export type ScreenPoint = { x: number; y: number };

export type PositionTarget = ScreenPoint | HTMLElement | Vector2 | Vector3;

export type BumblebeeScaleReference =
  HTMLElement | Vector2 | (() => HTMLElement | Vector2 | null);

export type MoveScrollIntoViewOptions = Pick<
  ScrollIntoViewOptions,
  "block" | "inline"
>;

export type MoveScrollIntoView = boolean | MoveScrollIntoViewOptions;

export type MoveTarget =
  | ScreenPoint
  | HTMLElement
  | Vector2
  | Vector3
  | {
      destination: PositionTarget;
      speed?: number;
      scalePercentage?: number;
      scaleReference?: BumblebeeScaleReference | null;
      curveIntensity?: number;
      scrollIntoView?: MoveScrollIntoView;
    };

export type MoveAlongOptions = {
  speed?: number;
  startScalePercentage?: number;
  scalePercentage?: number;
  scaleReference?: BumblebeeScaleReference | null;
};

export type BumblebeeStance = "standing" | "flying";

export type PlaceTarget =
  | PositionTarget
  | {
      destination: PositionTarget;
      scalePercentage?: number;
      scaleReference?: BumblebeeScaleReference | null;
      stance?: BumblebeeStance;
      visible?: boolean;
    };

export type BumblebeeAnchorMode =
  | boolean
  | {
      enabled?: boolean;
      scaleReference?: BumblebeeScaleReference | null;
      settleMs?: number;
      throttleMs?: number;
      lerpFactor?: number;
    };

export type BumblebeeOptions = {
  visible?: boolean;
  scaleReference?: BumblebeeScaleReference | null;
  anchorMode?: BumblebeeAnchorMode;
};

export type PuppetPosition = {
  // Runtime center position along the bottom edge, 0-100.
  horizontalPercent: number;
};

export type PuppetPose = {
  position?: PuppetPosition | null;
  scale?: number;
  occlusion?: number;
};

export type ChatPuppetOverlayOptions = PuppetPose;

export type PuppetImageMask = "none" | "circle";

export type PuppetShowOptions = {
  disableTilt?: boolean;
  preserveFacing?: boolean;
};

export type PuppetTransitionPhase = "show" | "hide";
export type PuppetTransitionSoundHandler = (
  phase: PuppetTransitionPhase,
) => void | Promise<void>;

export type PuppetOptions = {
  instanceId?: string;
  imageUrl?: string;
  imageMask?: PuppetImageMask;
  stickColor?: string;
  position?: PositionTarget;
  scale?: number;
  occlusion?: number;
  appearance?: "normal" | "ghost";
  visible?: boolean;
  hideAfterSpeech?: boolean;
  /** Let speech levels animate stick occlusion. Disable when another controller owns root positioning. */
  speechOcclusionMotion?: boolean;
  nameplate?: boolean | PuppetNameplate;
};

export type AdoptPuppetOptions = {
  puppetId: string;
  controller: PuppetModelController;
  nameplate?: PuppetNameplate | null;
  getChatBubbleScreenRect?: () => BubbleScreenRect | null;
  getChatBubbleViewport?: () => Viewport;
  getNameplateAnchor?: () => NameplateAnchor | null;
  onBeforeSpeak?: () => void | Promise<void>;
  onAfterSpeak?: () => void | Promise<void>;
};

export type AdoptActorOptions = {
  actorId: string;
  getChatBubbleScreenRect: () => BubbleScreenRect | null;
  getChatBubbleViewport?: () => Viewport;
  onBeforeSpeak?: () => void | Promise<void>;
  onAfterSpeak?: () => void | Promise<void>;
};

export type SpeechTarget =
  | { type: "bumblebee" }
  | {
      type: "puppet";
      id: string;
      actorId?: string;
      instanceId?: string;
      imageUrl?: string | null;
      imageMask?: PuppetImageMask | null;
      stickColor?: string | null;
      hideAfterSpeech?: boolean;
    }
  | { type: "actor"; actorId: string };

export type BubbleTarget = SpeechTarget | Bumblebee | Puppet;

export type BubbleMessage = {
  target: BubbleTarget;
  text: string;
  style?: ChatBubbleStyleTokens | null;
  durationMs?: number;
  autoReveal?: boolean;
  manualReveal?: boolean;
};

export type BubbleHandle = {
  id: string;
  reveal(
    text: string,
    options?: { chunkIndex?: number; fullCursor?: number },
  ): void;
  hide(options?: { immediate?: boolean }): Promise<void>;
};

export type SpeechBubbleFallback =
  | boolean
  | {
      durationMs?: number;
      autoReveal?: boolean;
    };

export type PreparedSpeechAudio = {
  url: string;
  words?: Array<{ text: string; startMs: number; durationMs: number }>;
  durationMs?: number;
};

export type Speech = {
  id?: string;
  visuals?: boolean;
  /** Prepared by the host; this package never calls speech providers. */
  audio?: PreparedSpeechAudio;
  target: SpeechTarget | Bumblebee | Puppet;
  /** Use Bumblebee's configured voice for an adopted actor or other visual target. */
  speaker?: "bumblebee";
  text?: string;
  file?: string;
  voiceId?: string;
  voice?: {
    voiceName: string;
    expression?: string;
    pitch?: string;
    rate?: string;
  };
  username?: string;
  handleId?: string;
  handleName?: string;
  stickColor?: string;
  chatBubbleStyleId?: string;
  chatBubbleStyle?: ChatBubbleStyleTokens | null;
  nameplateStyleId?: string;
  nameplateStyle?: NameplateStyleTokens | null;
  nameplateText?: string;
  platform?: NameplatePlatform;
  bubbles?: boolean;
  bubbleExitAnimation?: boolean;
  bubbleViewport?: Viewport | (() => Viewport);
  fallbackBubbles?: SpeechBubbleFallback;
  nameplates?: boolean;
  speechBubbleText?: string;
  speechTimeline?: SpeechTimeline | null;
  audioCues?: SpeechAudioCue[];
  audioBitcrusher?: {
    bitDepth?: number;
    sampleRateHz?: number;
    mix?: number;
    workletUrl: string;
    enabled?: boolean | (() => boolean);
  };
  audioFilter?: {
    highpassHz?: number;
    lowpassHz?: number;
    presence?: {
      frequencyHz?: number;
      gainDb?: number;
      q?: number;
    };
    compressor?: {
      thresholdDb?: number;
      kneeDb?: number;
      ratio?: number;
      attackSeconds?: number;
      releaseSeconds?: number;
    };
  };
};

export type SpeechAudioCue = {
  kind: "call_waiting" | VoiceChirpKind;
  afterText: string;
  volume?: number;
};

export type VoiceChirpKind = "wake_chirp" | "heard_chirp";

export type PuppetNameplate = {
  text: string;
  style?: NameplateStyleTokens | null;
  platform?: NameplatePlatform | null;
};

export type OverlayEvent =
  | { type: "ready" }
  | { type: "speech:start"; audioId: string; target: SpeechTarget }
  | { type: "speech:end"; audioId: string; target: SpeechTarget }
  | { type: "bubble:start"; audioId: string; target: SpeechTarget }
  | {
      type: "bubble:page-exit";
      audioId: string;
      pageId: string;
      chunkIndex: number;
      text: string;
    }
  | { type: "bubble:end"; audioId: string; target: SpeechTarget }
  | { type: "tool:presentation"; payload: ToolPresentationPayload }
  | { type: "media:play"; payload: OverlayMediaPlayPayload }
  | { type: "media:start"; payload: OverlayMediaStartPayload }
  | { type: "media:stop"; payload: OverlayMediaStopPayload }
  | {
      type: "actor:created";
      actorId: string;
      actorType: "bumblebee" | "puppet";
    }
  | {
      type: "actor:disposed";
      actorId: string;
      actorType: "bumblebee" | "puppet";
    }
  | { type: "runtime:disposed" };

export type OverlayEventType = OverlayEvent["type"];

export type OverlayEventFor<T extends OverlayEventType> = Extract<
  OverlayEvent,
  { type: T }
>;

export type EventUnsubscribe = () => void;
export type OverlayEventSubscriptionOptions = {
  replay?: boolean;
};

export type BumblebeeEmote =
  | "mad"
  | "mischief"
  | "roundEyes"
  | "sad"
  | "smileEyes"
  | "smileEyesU"
  | "talking"
  | "winkLeft"
  | "winkRight";

export type BumblebeeAttachmentPoint = "feet";

export type SoundEffect =
  | "bleep"
  | "bubblePop"
  | "error"
  | "message"
  | "perk"
  | "plink"
  | "startup"
  | "techTyping"
  | "transitionWhoosh"
  | "zoom";

export type SoundEffectPlaybackOptions = {
  playbackRate?: number;
  reverse?: boolean;
  bitcrusher?: {
    bitDepth?: number;
    sampleRateHz?: number;
    mix?: number;
  };
};

export type Overlay = {
  getCanvas(): HTMLCanvasElement | null;
  readonly actorRef: AnyActorRef;
  readonly audioContext: AudioContext;
  readonly audioOutput: GainNode;
  puppetImageUrl(id: string): string;
  bumblebee(options?: BumblebeeOptions): Promise<Bumblebee>;
  puppet(id: string, options?: PuppetOptions): Promise<Puppet>;
  setSurfaceContainer(container?: HTMLElement | null): HTMLCanvasElement | null;
  adoptActor(options: AdoptActorOptions): EventUnsubscribe;
  adoptPuppet(options: AdoptPuppetOptions): EventUnsubscribe;
  configureChatPuppets(
    update?: ChatPuppetOverlayOptions,
    options?: { animate?: boolean; durationMs?: number },
  ): Promise<void>;
  setChatPuppetSurfaceContainer(
    container?: HTMLElement | null,
  ): HTMLCanvasElement | null;
  setChatPuppetDecorationContainer(container?: HTMLElement | null): void;
  setSurfaceEffect(
    target: OverlaySurfaceTarget,
    effect: OverlaySurfaceEffect,
  ): void;
  showStreamerVoiceBubblePreview(
    options: StreamerVoiceBubblePreviewOptions,
  ): void;
  refreshStreamerVoiceBubblePreview(
    options: StreamerVoiceBubblePreviewOptions,
  ): void;
  hideStreamerVoiceBubblePreview(options?: {
    immediate?: boolean;
  }): Promise<void>;
  showBubble(message: BubbleMessage): Promise<BubbleHandle | null>;
  getBubblePage(audioId: string): ChatBubblePage | null;
  updateBubble(audioId: string, options: BubblePresentationUpdate): void;
  say(speech: Speech): Promise<void>;
  interruptSpeech(reason?: string): void;
  playSound(
    effect: SoundEffect,
    volume?: number,
    options?: SoundEffectPlaybackOptions,
  ): Promise<AudioBufferSourceNode>;
  playCallWaitingChime(volume?: number): Promise<void>;
  playVoiceChirp(kind: VoiceChirpKind, volume?: number): Promise<void>;
  playThinkingSound(
    durationMs?: number,
    volume?: number,
  ): Promise<OscillatorNode[]>;
  preloadSounds(effects?: SoundEffect[]): Promise<void>;
  setVolume(value: number): void;
  setAutomaticAudioResumeEnabled(enabled: boolean): void;
  setPlaybackPaused(paused: boolean): Promise<void>;
  setBubbleOptions(options: BubbleOptions | boolean): void;
  setNameplateOptions(options: NameplateOptions | boolean): void;
  on<T extends OverlayEventType>(
    event: T,
    handler: (event: OverlayEventFor<T>) => void,
    options?: OverlayEventSubscriptionOptions,
  ): EventUnsubscribe;
  dispose(): Promise<void>;
};

export type Bumblebee = {
  readonly id: "bumblebee";
  readonly actorId: string;
  readonly actorRef: AnyActorRef;
  readonly node: TransformNode;
  show(): Promise<void>;
  hide(): Promise<void>;
  fly(): Promise<void>;
  land(): Promise<void>;
  placeAt(target: PlaceTarget): Promise<void>;
  moveTo(target: MoveTarget): Promise<void>;
  moveAlong(
    targets: PositionTarget[],
    options?: MoveAlongOptions,
  ): Promise<void>;
  setAnchorMode(mode: BumblebeeAnchorMode): void;
  say(textOrSpeech: string | Omit<Speech, "target">): Promise<void>;
  emote(
    name: BumblebeeEmote,
    duration?: number,
    loop?: boolean,
    signal?: AbortSignal,
    weight?: number,
  ): Promise<void>;
  backflip(): Promise<void>;
  startTalking(): void;
  stopTalking(): void;
  getScreenRect(): BubbleScreenRect | null;
  getScalePercentage(): number;
  getAttachmentPoint(
    name: BumblebeeAttachmentPoint,
  ): { x: number; y: number } | null;
  isAnchoredTo(element: HTMLElement | null | undefined): boolean;
  getLastAnchor(): HTMLElement | null;
  on<T extends OverlayEventType>(
    event: T,
    handler: (event: OverlayEventFor<T>) => void,
    options?: OverlayEventSubscriptionOptions,
  ): EventUnsubscribe;
  dispose(): Promise<void>;
};

export type Puppet = {
  readonly id: string;
  readonly actorId: string;
  readonly actorRef: AnyActorRef;
  readonly speechTarget: SpeechTarget;
  show(options?: PuppetShowOptions): Promise<void>;
  hide(): Promise<void>;
  moveTo(target: PositionTarget): Promise<void>;
  say(textOrSpeech: string | Omit<Speech, "target">): Promise<void>;
  setStickColor(color: string): void;
  setOpacity(value: number): void;
  setDimmed(enabled: boolean): void;
  setRenderGroupOffset(offset: number): void;
  setScale(scale: number): void;
  setPose(
    pose: PuppetPose,
    options?: { animate?: boolean; durationMs?: number },
  ): Promise<void>;
  setGhostMode(enabled: boolean): void;
  setNameplate(nameplate: PuppetNameplate | null): void;
  setTransitionSoundHandler(handler: PuppetTransitionSoundHandler | null): void;
  getLocalSize(): { width: number; height: number } | null;
  setFacing(
    facing: "auto" | "left" | "right",
    options?: { animate?: boolean; durationMs?: number },
  ): void;
  startTalking(): void;
  stopTalking(): void;
  on<T extends OverlayEventType>(
    event: T,
    handler: (event: OverlayEventFor<T>) => void,
    options?: OverlayEventSubscriptionOptions,
  ): EventUnsubscribe;
  dispose(): Promise<void>;
};
