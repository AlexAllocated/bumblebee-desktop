<script lang="ts">
  import { onMount } from "svelte";
  import Stage from "../../src/Stage.svelte";
  import {
    defaultOverlaySettings,
    cloneOverlaySettings,
    type OverlaySettings,
  } from "../../src/lib/overlay";
  import type { OverlayEvent, Chatter } from "../../src/lib/types";
  const key = "bumblebee-dashboard-ui-fixture-v2";
  const read = (): OverlaySettings =>
    JSON.parse(localStorage.getItem(key) ?? "null")?.overlaySettings ??
    cloneOverlaySettings(defaultOverlaySettings);
  let settings = $state(read());
  let width = $state(960),
    height = $state(540),
    status = $state("Loading model");
  const listeners = new Set<(event: OverlayEvent) => void>();
  const emit = (event: OverlayEvent) => {
    for (const cb of listeners) cb(event);
  };
  const subscribe = (cb: (event: OverlayEvent) => void) => {
    listeners.add(cb);
    cb({ type: "overlay_settings", settings: cloneOverlaySettings(settings) });
    return () => listeners.delete(cb);
  };
  const chatter: Chatter = {
    platform: "twitch",
    userId: "sample-1",
    displayName: "Sample viewer",
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
  };
  function speech(puppet: boolean) {
    emit({
      type: "speech",
      id: crypto.randomUUID(),
      chatter: puppet ? chatter : null,
      text: "These are the retained speech bubbles, with timing from the audio clock.",
      audio_path: "fixture.wav",
      words: [
        { text: "These", startMs: 0, durationMs: 400 },
        { text: "are", startMs: 400, durationMs: 400 },
        { text: "the", startMs: 800, durationMs: 400 },
        { text: "retained", startMs: 1200, durationMs: 500 },
        { text: "speech", startMs: 1700, durationMs: 450 },
        { text: "bubbles,", startMs: 2150, durationMs: 450 },
        { text: "with", startMs: 2600, durationMs: 300 },
        { text: "timing", startMs: 2900, durationMs: 450 },
        { text: "from", startMs: 3350, durationMs: 300 },
        { text: "the", startMs: 3650, durationMs: 350 },
        { text: "audio", startMs: 4000, durationMs: 400 },
        { text: "clock.", startMs: 4400, durationMs: 500 },
      ],
      audible: false,
      gain: 0,
    });
  }
  onMount(() => {
    const changed = (event: StorageEvent) => {
      if (event.key === key) {
        settings = read();
        emit({
          type: "overlay_settings",
          settings: cloneOverlaySettings(settings),
        });
      }
    };
    window.addEventListener("storage", changed);
    return () => window.removeEventListener("storage", changed);
  });
</script>

<header class="fixture-controls">
  <strong>Muted OBS renderer fixture</strong><label
    >Viewport<select
      onchange={(e) => {
        const size = e.currentTarget.value.split("x").map(Number);
        width = size[0];
        height = size[1];
      }}
      ><option>960x540</option><option>1280x720</option><option>800x600</option
      ></select
    ></label
  ><button onclick={() => speech(false)}>Bee speech</button><button
    onclick={() => speech(true)}>Chat speech</button
  ><button
    onclick={() =>
      emit({
        type: "voice_transcript",
        userId: "owner",
        isOwner: true,
        text: "A streamer caption from the explicit fixture.",
        final: true,
      })}>Streamer caption</button
  ><button
    onclick={() =>
      emit({
        type: "voice_transcript",
        userId: "owner",
        isOwner: true,
        text: "",
        final: true,
      })}>Revoke caption</button
  ><span role="status">{status}</span>
</header>
<div class="fixture-stage" style={`width:${width}px;height:${height}px`}>
  <Stage
    assetBase={new URL("/", location.href).href}
    muted={true}
    {settings}
    {subscribe}
    onReady={async (verify) => {
      try {
        await verify();
        status = "Model pixels verified";
      } catch (error) {
        status = String(error);
      }
    }}
    onError={(error) => (status = String(error))}
  />
</div>

<style>
  .fixture-controls {
    height: auto;
    padding: 12px;
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    align-items: center;
    justify-content: flex-start;
  }
  label {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  select {
    width: auto;
  }
  .fixture-stage {
    position: relative;
    margin: 20px;
    background: repeating-conic-gradient(#223037 0 25%, #1a252a 0 50%) 0/32px
      32px;
  }
</style>
