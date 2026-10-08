import { Vector2 } from "@babylonjs/core/Maths/math.vector";
import { createScene } from "./legacy/bumblebee/createScene";
import { loadModel } from "./legacy/bumblebee/loadModel";
import { createPuppet, type PuppetController } from "./legacy/puppet";
import { screenToWorld } from "./legacy/utils/screenToWorld";
import type { Chatter, OverlayEvent, OverlaySettings } from "../lib/types";

type Speech = Extract<OverlayEvent, { type: "speech" }>;
type Entry = {
  profile: Chatter;
  puppet: PuppetController;
  label: HTMLDivElement;
  seen: number;
};

/** The same frame-driven models and playback lifecycle run in OBS and in the desktop preview. */
export function createStage(
  container: HTMLElement,
  assetBase: string,
  muted = false,
) {
  const scene = createScene({
    container,
    id: `desktop-${crypto.randomUUID()}`,
    zIndex: 1,
  });
  const entries = new Map<string, Entry>();
  const pending = new Map<string, Promise<Entry | null>>();
  const generations = new Map<string, number>();
  let nextGeneration = 0;
  let settings: OverlaySettings = {
    beeX: 0.15,
    beeY: 0.82,
    beeScale: 0.3,
    beeVisible: true,
    puppetScale: 0.3,
    puppetHorizontal: 0.7,
    puppetOcclusion: 0.15,
    puppetsVisible: true,
    bubblesVisible: true,
  };
  let disposed = false;
  let bee: Awaited<ReturnType<typeof loadModel>> | null = null;
  let activeAudio: HTMLAudioElement | null = null;
  let playing = false;
  let playGeneration = 0;
  let cleanupSpeech: (() => void) | null = null;
  const queue: Speech[] = [];
  const received = new Set<string>();
  const bubble = document.createElement("div");
  bubble.className = "speech-bubble";
  bubble.hidden = true;
  container.appendChild(bubble);
  const presentation = document.createElement("div");
  presentation.className = "overlay-presentation";
  presentation.hidden = true;
  container.appendChild(presentation);
  const url = (path: string) => new URL(path, assetBase).toString();
  const key = (p: Chatter) => `${p.platform}:${p.userId}`;
  const image = (p: Chatter) =>
    url(
      p.imageHash
        ? `images/${p.imageHash}.png`
        : `puppets/images/${p.puppetId}.png`,
    );
  const placeBee = () => {
    if (!bee || disposed) return;
    const canvas = scene.getEngine().getRenderingCanvas()!;
    const r = canvas.getBoundingClientRect();
    bee.node.position.copyFrom(
      screenToWorld(
        bee.node,
        new Vector2(r.width * settings.beeX, r.height * settings.beeY),
        scene.activeCamera!,
        canvas,
        settings.beeScale,
      ),
    );
    bee.node.setEnabled(settings.beeVisible);
  };
  const horizontal = (index: number) =>
    Math.max(
      0.05,
      Math.min(0.95, settings.puppetHorizontal - 0.22 + (index % 6) * 0.088),
    ) * 100;
  function applySettings(next: OverlaySettings) {
    const showPuppets = next.puppetsVisible && !settings.puppetsVisible;
    settings = next;
    placeBee();
    let index = 0;
    for (const entry of entries.values()) {
      entry.puppet.setPuppetPosition({
        horizontalPercent: horizontal(index++),
      });
      entry.puppet.setScalePercentage(settings.puppetScale);
      entry.puppet.setOcclusionRatio(settings.puppetOcclusion);
      entry.puppet.node.setEnabled(settings.puppetsVisible);
      if (showPuppets) void entry.puppet.show();
      entry.label.hidden = !settings.puppetsVisible;
    }
    if (!settings.bubblesVisible) bubble.hidden = true;
  }
  const ready = loadModel({
    scene,
    modelUrl: url("models/bumblebee.cb67e11b.glb"),
  }).then(async (model) => {
    if (disposed) {
      model.node.dispose();
      throw new Error("Renderer was disposed before its model loaded");
    }
    bee = model;
    model.animationGroups.find((a) => a.name === "idleFlying")?.start(true);
    placeBee();
    await scene.whenReadyAsync();
    await new Promise<void>((resolve) =>
      scene.onAfterRenderObservable.addOnce(() => resolve()),
    );
  });
  // Keep failures visible even if the caller does not wait for readiness (for example OBS).
  void ready.catch((error) => {
    if (!disposed) console.error("Bumblebee model could not load", error);
  });
  const resize = new ResizeObserver(placeBee);
  resize.observe(container);

  async function ensure(profile: Chatter): Promise<Entry | null> {
    const id = key(profile);
    const old = entries.get(id);
    if (old && image(old.profile) === image(profile)) {
      old.profile = profile;
      old.label.textContent = profile.displayName;
      old.seen = performance.now();
      return old;
    }
    if (pending.has(id)) {
      await pending.get(id);
      const ready = entries.get(id);
      if (ready && image(ready.profile) === image(profile)) {
        ready.profile = profile;
        ready.label.textContent = profile.displayName;
        ready.seen = performance.now();
        return ready;
      }
    }
    // A chat burst cannot start unbounded parallel image decoding and mesh creation.
    if (disposed || pending.size >= 4) return null;
    const version = ++nextGeneration;
    generations.set(id, version);
    const operation = (async () => {
      const existing = entries.get(id);
      if (existing) {
        await existing.puppet.dispose();
        existing.label.remove();
        entries.delete(id);
      }
      if (entries.size >= 10) {
        const oldest = [...entries].sort((a, b) => a[1].seen - b[1].seen)[0];
        if (oldest) {
          entries.delete(oldest[0]);
          oldest[1].label.remove();
          void oldest[1].puppet.dispose();
        }
      }
      const horizontalPercent = horizontal(entries.size);
      const options = {
        scene,
        puppetId: profile.puppetId,
        imageUrl: image(profile),
        puppetPosition: { horizontalPercent },
        scalePercentage: settings.puppetScale,
        occlusionPercentage: settings.puppetOcclusion,
      };
      let puppet: PuppetController;
      try {
        puppet = await createPuppet(options);
      } catch {
        puppet = await createPuppet({
          ...options,
          imageUrl: url(`puppets/images/${profile.puppetId}.png`),
        });
      }
      if (disposed || generations.get(id) !== version) {
        await puppet.dispose();
        return null;
      }
      // Loads finish asynchronously; enforce the live limit again at insertion.
      if (entries.size >= 10) {
        const oldest = [...entries].sort((a, b) => a[1].seen - b[1].seen)[0];
        if (oldest) {
          entries.delete(oldest[0]);
          oldest[1].label.remove();
          void oldest[1].puppet.dispose();
        }
      }
      const label = document.createElement("div");
      label.className = "puppet-label";
      label.textContent = profile.displayName;
      container.appendChild(label);
      const entry = { profile, puppet, label, seen: performance.now() };
      entries.set(id, entry);
      label.hidden = !settings.puppetsVisible;
      if (settings.puppetsVisible) void puppet.show();
      else puppet.node.setEnabled(false);
      return entry;
    })().catch((error) => {
      if (!disposed) console.error("Puppet failed to render", error);
      return null;
    });
    pending.set(id, operation);
    try {
      return await operation;
    } finally {
      if (pending.get(id) === operation) {
        pending.delete(id);
        generations.delete(id);
      }
    }
  }
  const beforeRender = scene.onBeforeRenderObservable.add(() => {
    const rect = container.getBoundingClientRect();
    for (const entry of entries.values()) {
      const pose = entry.puppet.getNameplateScreenAnchor();
      if (pose) {
        entry.label.style.left = `${pose.x - rect.left}px`;
        entry.label.style.top = `${pose.y - rect.top}px`;
        entry.label.style.transform = `translate(-50%,-50%) rotate(${pose.rotationDeg}deg) scale(${pose.scale})`;
      }
    }
  });
  function stop() {
    playGeneration++;
    queue.length = 0;
    activeAudio?.pause();
    activeAudio = null;
    cleanupSpeech?.();
    cleanupSpeech = null;
    playing = false;
    bubble.hidden = true;
    for (const entry of entries.values()) void entry.puppet.shutup();
    bee?.animationGroups.find((a) => a.name === "talking")?.stop();
  }
  async function playNext() {
    if (playing || disposed) return;
    const event = queue.shift();
    if (!event) return;
    playing = true;
    const generation = playGeneration;
    const entry = event.chatter ? await ensure(event.chatter) : null;
    if (disposed || generation !== playGeneration) return;
    const mediaPath = event.audio_path.replace(/^media\//, "");
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,159}$/.test(mediaPath)) {
      playing = false;
      void playNext();
      return;
    }
    const audio = new Audio(url(`media/${mediaPath}`));
    activeAudio = audio;
    audio.muted = muted;
    bubble.hidden = !settings.bubblesVisible;
    bubble.textContent = event.text;
    bubble.style.left = event.chatter ? "55%" : "22%";
    let frame = 0;
    let speaking = false;
    let highlightedWord = -1;
    const animate = () => {
      if (disposed || generation !== playGeneration) return;
      const ms = audio.currentTime * 1000;
      const current = event.words.findIndex(
        (w) => ms >= w.startMs && ms < w.startMs + w.durationMs,
      );
      const talking =
        !audio.paused && (event.words.length === 0 || current >= 0);
      if (talking !== speaking) {
        speaking = talking;
        if (entry) {
          if (talking) void entry.puppet.talk();
          else void entry.puppet.shutup();
        } else {
          const group = bee?.animationGroups.find((a) => a.name === "talking");
          if (talking) group?.start(true);
          else group?.stop();
        }
      }
      if (event.words.length && current >= 0 && current !== highlightedWord) {
        highlightedWord = current;
        bubble.replaceChildren(
          ...event.words.map((w, i) => {
            const span = document.createElement("span");
            span.textContent = w.text + " ";
            span.className = i === current ? "current-word" : "";
            return span;
          }),
        );
      }
      frame = requestAnimationFrame(animate);
    };
    const finish = () => {
      cancelAnimationFrame(frame);
      audio.onended = null;
      audio.onerror = null;
      if (entry) void entry.puppet.shutup();
      else bee?.animationGroups.find((a) => a.name === "talking")?.stop();
      if (generation !== playGeneration) return;
      activeAudio = null;
      cleanupSpeech = null;
      playing = false;
      bubble.hidden = true;
      void playNext();
    };
    cleanupSpeech = () => {
      cancelAnimationFrame(frame);
      audio.onended = null;
      audio.onerror = null;
    };
    audio.onended = finish;
    audio.onerror = finish;
    try {
      await audio.play();
      if (generation === playGeneration) animate();
      else audio.pause();
    } catch (error) {
      console.warn("Audio playback was blocked or unavailable", error);
      finish();
    }
  }
  return {
    ready,
    async consume(event: OverlayEvent) {
      if (disposed) return;
      if (event.type === "overlay_settings") {
        applySettings(event.settings);
        return;
      }
      if (event.type === "stop_speech") {
        stop();
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
        presentation.appendChild(title);
        if (event.type === "image") {
          const file = event.image_path.replace(/^media\//, "");
          if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,159}$/.test(file)) {
            presentation.hidden = true;
            return;
          }
          const image = document.createElement("img");
          image.alt = event.title;
          image.src = url(`media/${file}`);
          presentation.appendChild(image);
        } else {
          const text = document.createElement("p");
          text.textContent = event.text.slice(0, 6000);
          presentation.appendChild(text);
        }
        presentation.hidden = false;
        return;
      }
      if (event.type === "chat" || event.type === "chatter_changed") {
        if (event.type === "chat" || entries.has(key(event.chatter)))
          await ensure(event.chatter);
      } else if (event.type === "speech" && !received.has(event.id)) {
        received.add(event.id);
        if (received.size > 256)
          received.delete(received.values().next().value!);
        if (queue.length >= 8) queue.shift();
        queue.push(event);
        void playNext();
      }
    },
    setMuted(value: boolean) {
      muted = value;
      if (activeAudio) activeAudio.muted = value;
    },
    dispose() {
      disposed = true;
      stop();
      resize.disconnect();
      scene.onBeforeRenderObservable.remove(beforeRender);
      for (const entry of entries.values()) {
        entry.label.remove();
        void entry.puppet.dispose();
      }
      entries.clear();
      bubble.remove();
      presentation.remove();
      const engine = scene.getEngine();
      const canvas = engine.getRenderingCanvas();
      scene.dispose();
      engine.dispose();
      canvas?.remove();
    },
  };
}
