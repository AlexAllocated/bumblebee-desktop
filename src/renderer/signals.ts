import type { AudioMix, OverlayEvent } from "../lib/types";
import { signalGain } from "./audioMix";
type Signal = Extract<OverlayEvent, { type: "signal" }>;

/** Independent cue clock; stopping a thinking tone never cancels a spoken clip. */
export function createSignals(
  assetBase: string,
  context: () => AudioContext,
  mix: () => AudioMix | null,
  muted: () => boolean,
) {
  type Playing = {
    event: Signal;
    audio: HTMLAudioElement;
    source: MediaElementAudioSourceNode | null;
    gain: GainNode | null;
  };
  const playing = new Map<string, Playing>();
  const received = new Set<string>();
  let disposed = false;
  function stop(id: string) {
    const item = playing.get(id);
    if (!item) return;
    playing.delete(id);
    item.audio.pause();
    item.audio.onended = null;
    item.audio.onerror = null;
    item.source?.disconnect();
    item.gain?.disconnect();
  }
  function update() {
    for (const item of playing.values()) {
      const gain = signalGain(item.event, mix(), muted());
      if (item.gain) item.gain.gain.value = gain;
      else item.audio.volume = Math.min(1, gain);
    }
  }
  async function start(event: Signal) {
    if (disposed) return;
    const previous = playing.get(event.id);
    if (previous) {
      previous.event = event;
      update();
      return;
    }
    if (received.has(event.id)) return;
    const file = event.audio_path.replace(/^media\//, "");
    if (!/^[A-Za-z0-9][A-Za-z0-9._-]{0,159}$/.test(file)) return;
    received.add(event.id);
    if (received.size > 256) received.delete(received.values().next().value!);
    if (playing.size >= 4) stop(playing.keys().next().value!);
    const audio = new Audio();
    audio.crossOrigin = "anonymous";
    audio.src = new URL(`media/${file}`, assetBase).toString();
    audio.loop = event.looping;
    const item: Playing = { event, audio, source: null, gain: null };
    playing.set(event.id, item);
    try {
      const ctx = context();
      item.source = ctx.createMediaElementSource(audio);
      item.gain = ctx.createGain();
      item.source.connect(item.gain);
      item.gain.connect(ctx.destination);
      if (ctx.state === "suspended") void ctx.resume().catch(() => {});
    } catch (error) {
      console.warn("Cue gain processing unavailable", error);
    }
    update();
    audio.onended = () => stop(event.id);
    audio.onerror = () => stop(event.id);
    try {
      await audio.play();
      if (disposed || playing.get(event.id) !== item) audio.pause();
    } catch {
      stop(event.id);
    }
  }
  return {
    start,
    stop,
    update,
    dispose() {
      disposed = true;
      for (const id of [...playing.keys()]) stop(id);
      received.clear();
    },
  };
}
