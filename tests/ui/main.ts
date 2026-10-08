/** Explicit isolated UI fixture. No provider calls, credentials, or production persistence. */
import { mount } from "svelte";
import App from "../../src/App.svelte";
import "../../src/style.css";
import {
  applyOverlayPatch,
  defaultOverlaySettings,
} from "../../src/lib/overlay";
import { mergeSettings } from "../../src/lib/settingsWriter";
import type { DesktopBackend } from "../../src/lib/desktop";
import type { Snapshot, OverlayEvent } from "../../src/lib/types";
import settings from "./settings.json";
import puppets from "../../assets/catalog/puppets.json";
import voiceCatalog from "../../assets/catalog/voices.json";
const voices = voiceCatalog.map((v) => ({
  ...v,
  role: v.role ?? "puppet",
  provider: "azure_speech",
}));
const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v));
const key = "bumblebee-dashboard-ui-fixture-v2";
const saved = JSON.parse(localStorage.getItem(key) ?? "null");
let state: Snapshot = {
  settings: saved?.settings ?? settings,
  overlaySettings: saved?.overlaySettings ?? clone(defaultOverlaySettings),
  puppets,
  voices,
  pendingImages: [],
  chatters: [
    {
      platform: "twitch",
      userId: "viewer-1",
      displayName: "Sample viewer",
      puppetId: "alopex",
      voiceId: voices.find((v) => v.role === "puppet")!.id,
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
  ],
  overlayUrl: new URL("/overlay.html", location.href).href,
  secrets: {
    azure_speech: true,
    openai: true,
    discord_bot: true,
    twitch_tokens: true,
    google_tokens: true,
  },
  statuses: [],
  active: false,
  credentialStoreError: null,
  overlayError: null,
  pendingInputs: [],
  interruptedTurns: [],
  artifacts: [],
};
const listeners = new Map<string, Set<(event: any) => void>>();
const emit = (payload: OverlayEvent) => {
  for (const callback of listeners.get("bumblebee:overlay") ?? [])
    callback({ payload });
};
const persist = () =>
  localStorage.setItem(
    key,
    JSON.stringify({
      settings: state.settings,
      overlaySettings: state.overlaySettings,
    }),
  );
const backend: DesktopBackend = {
  available: true,
  invoke: async (command, args: any = {}) => {
    await new Promise((r) => setTimeout(r, 100));
    switch (command) {
      case "get_snapshot":
        return clone(state) as any;
      case "get_activity":
        return clone({
          pendingInputs: [],
          interruptedTurns: [],
          pendingImages: [],
          active: state.active,
          artifacts: [],
        }) as any;
      case "patch_settings":
        state.settings = mergeSettings(state.settings, args.patch);
        persist();
        return clone(state.settings) as any;
      case "patch_overlay_settings":
        state.overlaySettings = applyOverlayPatch(
          state.overlaySettings,
          args.patch,
        );
        persist();
        emit({
          type: "overlay_settings",
          settings: clone(state.overlaySettings),
        });
        return clone(state.overlaySettings) as any;
      case "discord_guilds":
        return [
          { id: "10", name: "Test server", kind: 0, parentId: null },
        ] as any;
      case "discord_options":
        return {
          channels: [{ id: "20", name: "Live", kind: 2, parentId: null }],
          roles: [{ id: "30", name: "Stream guests", kind: 0, parentId: null }],
        } as any;
      case "search_chatters":
        return clone(state.chatters) as any;
      case "update_chatter":
        state.chatters = state.chatters.map((c) =>
          c.userId === args.userId ? { ...c, ...args } : c,
        );
        return undefined as any;
      case "get_library":
        return { memories: [], reminders: [] } as any;
      case "openai_models":
        return ["gpt-5.4-mini"] as any;
      case "start_session":
        state.active = true;
        return undefined as any;
      case "stop_session":
        state.active = false;
        return undefined as any;
      case "cancel_speech":
        emit({ type: "stop_speech" });
        return undefined as any;
      case "preview_speech": {
        // The fixture supplies silent PCM and explicit timings; it never synthesizes.
        const text = String(args.text ?? "").slice(0, 6000);
        const words = text.trim().split(/\s+/).filter(Boolean);
        const durationMs = 5000 / Math.max(1, words.length);
        emit({
          type: "speech",
          id: crypto.randomUUID(),
          chatter: null,
          text,
          audio_path: "fixture.wav",
          words: words.map((text, index) => ({
            text,
            startMs: index * durationMs,
            durationMs,
          })),
          audible: false,
          gain: 0,
        });
        return undefined as any;
      }
      default:
        throw new Error(`UI fixture has no provider operation: ${command}`);
    }
  },
  listen: async (name, callback) => {
    let set = listeners.get(name);
    if (!set) listeners.set(name, (set = new Set()));
    set.add(callback);
    return () => set!.delete(callback);
  },
  openUrl: async () => {
    throw new Error("External links disabled in UI fixture");
  },
  autostart: {
    enable: async () => {},
    disable: async () => {},
    isEnabled: async () => false,
  },
};
mount(App, { target: document.getElementById("app")!, props: { backend } });
