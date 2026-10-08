import { Vector2 } from "@babylonjs/core/Maths/math.vector";
import {
  createOverlay,
  type Overlay,
  type Bumblebee,
  type Puppet,
  type SpeechTarget,
} from "@hivetech/bumblebee";
import {
  createBubbleRenderer,
  type BubbleRenderer,
} from "@hivetech/speech-bubbles";
import type { AudioMix, Chatter, OverlayEvent } from "../lib/types";
import {
  cloneOverlaySettings,
  defaultOverlaySettings,
  type OverlaySettings,
} from "../lib/overlay";
import {
  subjectRect,
  streamerPose,
  defaultDimensions,
  type Dimensions,
} from "./overlayLayout";
import { bumblebeePlacement } from "./legacyLayout";
import { chatBubbleStyleSeeds } from "./bubbleStyles";
import { speechGain } from "./audioMix";
import { createSignals } from "./signals";

type Speech = Extract<OverlayEvent, { type: "speech" }>;
type Entry = {
  profile: Chatter;
  actor: Puppet;
  seen: number;
  preview: boolean;
  visible: boolean;
};
/** Native transport adapter. The reusable package owns actors, speech, its queue, and its clock. */
export function createStage(
  container: HTMLElement,
  assetBase: string,
  muted = false,
  editing = false,
) {
  let runtime: Overlay | null = null,
    bee: Bumblebee | null = null,
    disposed = false;
  let settings = cloneOverlaySettings(defaultOverlaySettings);
  let dimensions: Dimensions = structuredClone(defaultDimensions);
  let audioMix: AudioMix | null = null,
    signalContext: AudioContext | null = null;
  let activeSpeech: Speech | null = null,
    lastChatKey = "";
  let speechGeneration = 0;
  const entries = new Map<string, Entry>(),
    pending = new Map<string, Promise<Entry | null>>();
  const speechMetadata = new Map<string, Speech>(),
    received = new Set<string>();
  const viewport = () => ({
    width: container.clientWidth,
    height: container.clientHeight,
  });
  const url = (path: string) => new URL(path, assetBase).toString();
  const identity = (profile: Chatter) =>
    `${profile.platform}:${profile.userId}`;
  const image = (profile: Chatter) =>
    url(
      profile.imageHash
        ? `images/${profile.imageHash}.png`
        : `puppets/images/${profile.puppetId}.png`,
    );
  const style = (id: string) =>
    (chatBubbleStyleSeeds.find((s) => s.id === id) ?? chatBubbleStyleSeeds[0])
      .style;
  const signals = createSignals(
    assetBase,
    () => (signalContext ??= new AudioContext()),
    () => audioMix,
    () => muted,
  );
  let streamerCaption: BubbleRenderer | null = null,
    streamerPreview: BubbleRenderer | null = null;
  let streamerTimer: ReturnType<typeof setTimeout> | undefined;
  const presentation = document.createElement("div");
  presentation.className = "overlay-presentation";
  presentation.hidden = true;
  container.appendChild(presentation);
  function report(error: unknown) {
    if (!disposed) console.error("Overlay operation failed", error);
  }
  function placeBee() {
    if (!bee || disposed) return;
    const v = viewport(),
      s = settings.bumblebee;
    const placement = bumblebeePlacement(s, v, dimensions.bee);
    const bounds = subjectRect(settings, "bumblebee", v, dimensions);
    void bee
      .placeAt({
        destination: new Vector2(placement.x, placement.y),
        scalePercentage: s.scalePercentage,
        scaleReference: new Vector2(v.width, v.height),
        stance: bounds.bottom >= v.height * 0.97 ? "standing" : "flying",
        visible: s.visible,
      })
      .catch(report);
  }
  function chatPose() {
    const v = viewport(),
      bounds = subjectRect(settings, "puppet", v, dimensions);
    return {
      position: {
        horizontalPercent: (bounds.centerX / Math.max(1, v.width)) * 100,
      },
      scale: settings.puppet.scalePercentage,
      occlusion: settings.puppet.occlusionPercentage,
    };
  }
  function applyEntry(entry: Entry) {
    const s = settings.puppet;
    const current = activeSpeech?.chatter
      ? identity(activeSpeech.chatter)
      : null;
    const visible =
      s.enabled &&
      (entry.preview
        ? editing && settings.editPreview.puppet && !current
        : identity(entry.profile) === current ||
          (!activeSpeech &&
            s.showWhenIdle &&
            identity(entry.profile) === lastChatKey));
    void entry.actor.setPose(chatPose(), { animate: false }).catch(report);
    entry.actor.setNameplate(
      visible && !entry.preview && s.nameplatesEnabled
        ? {
            text: entry.profile.displayName,
            platform: entry.profile.platform as
              "discord" | "twitch" | "youtube",
          }
        : null,
    );
    if (visible !== entry.visible) {
      entry.visible = visible;
      void (visible ? entry.actor.show() : entry.actor.hide()).catch(report);
    }
  }
  function applyBubbleOptions() {
    const s = activeSpeech?.chatter ? settings.puppet : settings.bumblebee;
    runtime?.setBubbleOptions({
      enabled: s.chatBubblesEnabled,
      container,
      viewport,
      maxWidthPercent: s.chatBubbleMaxWidthPercent,
      maxHeightPercent: s.chatBubbleMaxHeightPercent,
      scale: s.chatBubbleTextSizePercentage,
      zIndex: 4,
    });
    runtime?.setNameplateOptions({
      enabled: settings.puppet.nameplatesEnabled,
      container,
      zIndex: 5,
    });
  }
  function updateVolume() {
    if (runtime)
      runtime.setVolume(
        activeSpeech
          ? speechGain(activeSpeech, audioMix, muted)
          : muted || audioMix?.output === "discord"
            ? 0
            : (audioMix?.masterVolume ?? 1),
      );
    signals.update();
  }
  function streamerOptions() {
    const s = settings.streamerVoiceBubble,
      body = subjectRect(settings, "streamerVoiceBubble", viewport());
    return {
      target: container,
      pose: streamerPose(settings, viewport()),
      style: style(s.chatBubbleStyleId),
      scale: s.textSizePercentage / 0.5,
      fixedBodyRect: body,
      fixedBodySize: { width: body.width, height: body.height },
      maxWidthPercent: s.chatBubbleMaxWidthPercent,
      maxHeightPercent: s.chatBubbleMaxHeightPercent,
      viewport,
      zIndex: 4,
    };
  }
  function refreshCaption() {
    if (!settings.streamerVoiceBubble.enabled) {
      clearTimeout(streamerTimer);
      streamerCaption?.dispose();
      streamerCaption = null;
    } else streamerCaption?.update(streamerOptions());
    streamerPreview?.dispose();
    streamerPreview = null;
    if (
      editing &&
      settings.streamerVoiceBubble.enabled &&
      settings.editPreview.streamerVoiceBubble &&
      !streamerCaption
    ) {
      streamerPreview = createBubbleRenderer({
        ...streamerOptions(),
        text: "Streamer voice bubble",
      });
      streamerPreview.show();
    }
  }
  async function ensure(
    profile: Chatter,
    preview = false,
  ): Promise<Entry | null> {
    if (!runtime || disposed) return null;
    const key = identity(profile),
      existing = entries.get(key);
    if (existing && image(existing.profile) === image(profile)) {
      existing.profile = profile;
      existing.seen = performance.now();
      return existing;
    }
    if (pending.has(key)) {
      await pending.get(key);
      return ensure(profile, preview);
    }
    while (!disposed && pending.size >= 4) await Promise.race(pending.values());
    if (disposed) return null;
    const operation = (async () => {
      if (existing) {
        entries.delete(key);
        await existing.actor.dispose();
      }
      const options = {
        instanceId: key,
        imageUrl: image(profile),
        visible: false,
        hideAfterSpeech: !settings.puppet.showWhenIdle,
        scale: settings.puppet.scalePercentage,
        occlusion: settings.puppet.occlusionPercentage,
      };
      let actor: Puppet;
      try {
        actor = await runtime!.puppet(profile.puppetId, options);
      } catch {
        actor = await runtime!.puppet(profile.puppetId, {
          ...options,
          imageUrl: url(`puppets/images/${profile.puppetId}.png`),
        });
      }
      if (disposed) {
        await actor.dispose();
        return null;
      }
      if (entries.size >= 28) {
        const oldest = [...entries]
          .filter(
            ([, e]) =>
              identity(e.profile) !==
                (activeSpeech?.chatter
                  ? identity(activeSpeech.chatter)
                  : null) && !e.preview,
          )
          .sort((a, b) => a[1].seen - b[1].seen)[0];
        if (oldest) {
          entries.delete(oldest[0]);
          await oldest[1].actor.dispose();
        }
      }
      const entry = {
        profile,
        actor,
        seen: performance.now(),
        preview,
        visible:
          !preview &&
          settings.puppet.enabled &&
          identity(profile) ===
            (activeSpeech?.chatter ? identity(activeSpeech.chatter) : null),
      };
      entries.set(key, entry);
      if (!preview) {
        const d = actor.getLocalSize();
        if (d) dimensions.puppetAspect = d.width / d.height;
      }
      applyEntry(entry);
      return entry;
    })().catch((error) => {
      report(error);
      return null;
    });
    pending.set(key, operation);
    try {
      return await operation;
    } finally {
      if (pending.get(key) === operation) pending.delete(key);
    }
  }
  let previewSync: Promise<void> | null = null;
  function syncPreview() {
    if (previewSync || disposed || !runtime) return;
    previewSync = (async () => {
      if (editing && settings.editPreview.puppet)
        await ensure(
          {
            platform: "preview",
            userId: "chat",
            displayName: "Preview",
            puppetId: "dandy",
            voiceId: "dandy",
            imageHash: null,
            customizationBlocked: false,
            overrides: {
              chatPuppet: "inherit",
              relay: "inherit",
              ttsWait: "inherit",
              aiAccess: "inherit",
              textModel: null,
              voiceModel: null,
            },
          },
          true,
        );
      const preview = entries.get("preview:chat");
      if (preview) applyEntry(preview);
    })().finally(() => {
      previewSync = null;
    });
  }
  function applySettings(next: OverlaySettings) {
    settings = cloneOverlaySettings(next);
    placeBee();
    void runtime
      ?.configureChatPuppets(chatPose(), { animate: false })
      .catch(report);
    for (const entry of entries.values()) applyEntry(entry);
    applyBubbleOptions();
    refreshCaption();
    syncPreview();
  }
  const ready = (async () => {
    const created = await createOverlay({
      assetBaseUrl: assetBase,
      assets: {
        bumblebeeModelUrl: url("models/bumblebee.cb67e11b.glb"),
        puppetImageUrl: (id) => url(`puppets/images/${id}.png`),
      },
      surface: { container },
      zIndex: 1,
      bubbles: { container, viewport, zIndex: 4 },
      nameplates: { container, zIndex: 5 },
      chatPuppets: chatPose(),
    });
    if (disposed) {
      await created.dispose();
      throw new Error("Renderer disposed before initialization");
    }
    runtime = created;
    updateVolume();
    runtime.on("actor:disposed", (event) => {
      for (const [key, entry] of entries) {
        if (entry.actor.actorId === event.actorId) entries.delete(key);
      }
    });
    runtime.on("speech:start", (event) => {
      activeSpeech = speechMetadata.get(event.audioId) ?? null;
      updateVolume();
      applyBubbleOptions();
      if (activeSpeech?.chatter) {
        lastChatKey = identity(activeSpeech.chatter);
        void ensure(activeSpeech.chatter).then((entry) => {
          if (entry && !disposed) applyEntry(entry);
        });
      }
      for (const entry of entries.values()) applyEntry(entry);
    });
    runtime.on("speech:end", (event) => {
      speechMetadata.delete(event.audioId);
      if (activeSpeech?.id === event.audioId) {
        if (activeSpeech.chatter && !settings.puppet.showWhenIdle) {
          const entry = entries.get(identity(activeSpeech.chatter));
          if (entry) entry.visible = false;
        }
        activeSpeech = null;
      }
      for (const entry of entries.values()) applyEntry(entry);
      updateVolume();
    });
    bee = await runtime.bumblebee({
      visible: settings.bumblebee.visible,
      scaleReference: container,
    });
    if (disposed) {
      await runtime.dispose();
      return;
    }
    dimensions.bee = bee.node.metadata.localDims;
    applySettings(settings);
    const scene = bee.node.getScene();
    await scene.whenReadyAsync();
    await new Promise<void>((resolve) =>
      scene.onAfterRenderObservable.addOnce(() => resolve()),
    );
  })();
  void ready.catch(report);
  const resize = new ResizeObserver(() => {
    // screenToWorld uses the camera's engine aspect ratio as well as our local
    // viewport. Update that ratio before computing the responsive placement.
    bee?.node.getScene().getEngine().resize();
    placeBee();
    for (const e of entries.values()) applyEntry(e);
    refreshCaption();
    applyBubbleOptions();
  });
  resize.observe(container);
  function stop() {
    speechGeneration++;
    speechMetadata.clear();
    activeSpeech = null;
    runtime?.interruptSpeech("desktop cancellation");
    for (const e of entries.values()) applyEntry(e);
    updateVolume();
  }
  async function consume(event: OverlayEvent) {
    if (disposed) return;
    if (event.type === "overlay_settings") {
      applySettings(event.settings);
      return;
    }
    if (event.type === "audio_settings") {
      audioMix = { ...event.settings };
      updateVolume();
      return;
    }
    if (event.type === "stop_speech") {
      stop();
      return;
    }
    if (event.type === "signal") {
      await signals.start(event);
      return;
    }
    if (event.type === "stop_signal") {
      signals.stop(event.id);
      return;
    }
    if (event.type === "voice_transcript") {
      if (!event.isOwner) return;
      clearTimeout(streamerTimer);
      streamerCaption?.dispose();
      streamerCaption = null;
      if (!event.text.trim() || !settings.streamerVoiceBubble.enabled) {
        refreshCaption();
        return;
      }
      streamerPreview?.dispose();
      streamerPreview = null;
      streamerCaption = createBubbleRenderer({
        ...streamerOptions(),
        text: event.text,
      });
      streamerCaption.show();
      streamerTimer = setTimeout(() => {
        streamerCaption?.dispose();
        streamerCaption = null;
        refreshCaption();
      }, 6000);
      return;
    }
    if (event.type === "hide_image") {
      presentation.replaceChildren();
      presentation.hidden = true;
      return;
    }
    if (event.type === "image" || event.type === "presentation") {
      presentation.replaceChildren();
      const title = document.createElement("h2");
      title.textContent = event.title.slice(0, 200);
      presentation.append(title);
      if (event.type === "image") {
        const file = event.image_path.replace(/^media\//, "");
        if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,159}$/.test(file)) {
          presentation.hidden = true;
          return;
        }
        const img = document.createElement("img");
        img.alt = event.title;
        img.src = url(`media/${file}`);
        presentation.append(img);
      } else {
        const text = document.createElement("p");
        text.textContent = event.text.slice(0, 6000);
        presentation.append(text);
      }
      presentation.hidden = false;
      return;
    }
    const generation = speechGeneration;
    await ready;
    if (disposed || generation !== speechGeneration) return;
    if (event.type === "chat" || event.type === "chatter_changed") {
      if (event.type === "chat") lastChatKey = identity(event.chatter);
      if (settings.puppet.showWhenIdle || entries.has(identity(event.chatter)))
        await ensure(event.chatter);
      for (const entry of entries.values()) applyEntry(entry);
      return;
    }
    if (event.type !== "speech" || received.has(event.id)) return;
    const file = event.audio_path.replace(/^media\//, "");
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,159}$/.test(file)) return;
    received.add(event.id);
    if (received.size > 256) received.delete(received.values().next().value!);
    const s = event.chatter ? settings.puppet : settings.bumblebee;
    const target: SpeechTarget = event.chatter
      ? {
          type: "puppet",
          id: event.chatter.puppetId,
          instanceId: identity(event.chatter),
          imageUrl: image(event.chatter),
          hideAfterSpeech: !settings.puppet.showWhenIdle,
        }
      : { type: "bumblebee" };
    speechMetadata.set(event.id, event);
    void runtime!
      .say({
        id: event.id,
        target,
        text: event.text,
        audio: { url: url(`media/${file}`), words: event.words },
        visuals: event.chatter
          ? settings.puppet.enabled
          : settings.bumblebee.visible,
        bubbles: s.chatBubblesEnabled,
        nameplates: !!event.chatter && settings.puppet.nameplatesEnabled,
        nameplateText: event.chatter?.displayName,
        platform: event.chatter?.platform as
          "discord" | "twitch" | "youtube" | undefined,
        chatBubbleStyle: style(s.chatBubbleStyleId),
      })
      .catch(report)
      .finally(() => speechMetadata.delete(event.id));
  }
  return {
    ready,
    consume,
    applySettings,
    getDimensions: () => structuredClone(dimensions),
    async verifyRenderedFrame() {
      await ready;
      if (disposed || !bee)
        throw new Error("Renderer disposed before verification");
      const scene = bee.node.getScene(),
        engine = scene.getEngine();
      scene.render();
      const pixels = await engine.readPixels(
        0,
        0,
        engine.getRenderWidth(),
        engine.getRenderHeight(),
      );
      const bytes = new Uint8Array(
        pixels.buffer,
        pixels.byteOffset,
        pixels.byteLength,
      );
      let visible = 0;
      for (let i = 3; i < bytes.length; i += 4) if (bytes[i] > 0) visible++;
      if (visible < 20)
        throw new Error("The bee canvas rendered no visible model pixels");
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      );
    },
    setMuted(value: boolean) {
      muted = value;
      updateVolume();
    },
    setEditing(value: boolean) {
      if (editing === value) return;
      editing = value;
      for (const entry of entries.values()) applyEntry(entry);
      refreshCaption();
      syncPreview();
    },
    dispose() {
      disposed = true;
      stop();
      resize.disconnect();
      signals.dispose();
      clearTimeout(streamerTimer);
      streamerCaption?.dispose();
      streamerPreview?.dispose();
      presentation.remove();
      entries.clear();
      void signalContext?.close();
      void runtime?.dispose().catch(report);
    },
  };
}
