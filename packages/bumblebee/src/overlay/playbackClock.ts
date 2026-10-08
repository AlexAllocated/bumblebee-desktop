import type { Scene } from "@babylonjs/core/scene";
import {
  PausableScheduler,
  type PausableTimer,
} from "@hivetech/speech-bubbles";

export class PlaybackClock {
  readonly #scheduler = new PausableScheduler();

  get paused() {
    return this.#scheduler.paused;
  }

  now() {
    return this.#scheduler.now();
  }

  setPaused(paused: boolean) {
    this.#scheduler.setPaused(paused);
  }

  setTimeout(callback: () => void, delayMs: number) {
    return this.#scheduler.setTimeout(callback, delayMs);
  }

  clearTimeout(timer: PausableTimer | null | undefined) {
    this.#scheduler.clearTimeout(timer);
  }

  dispose() {
    this.#scheduler.dispose();
  }
}

const sceneClocks = new WeakMap<Scene, PlaybackClock>();

export const attachPlaybackClock = (scene: Scene, clock: PlaybackClock) => {
  sceneClocks.set(scene, clock);
};

export const playbackNow = (scene: Scene) =>
  sceneClocks.get(scene)?.now() ??
  (typeof performance !== "undefined" && typeof performance.now === "function"
    ? performance.now()
    : Date.now());

export const playbackPaused = (scene: Scene) =>
  sceneClocks.get(scene)?.paused ?? false;

export const playbackSetTimeout = (
  scene: Scene,
  callback: () => void,
  delayMs: number,
): PausableTimer | ReturnType<typeof setTimeout> =>
  sceneClocks.get(scene)?.setTimeout(callback, delayMs) ??
  setTimeout(callback, delayMs);
