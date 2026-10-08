<script lang="ts">
  import { onMount } from "svelte";
  import { createStage } from "./renderer/stage";
  import OverlayEditor from "./OverlayEditor.svelte";
  import {
    cloneOverlaySettings,
    defaultOverlaySettings,
    applyOverlayPatch,
    type OverlaySettings,
    type OverlaySettingsPatch,
  } from "./lib/overlay";
  import { defaultDimensions } from "./renderer/overlayLayout";
  import type { OverlayEvent } from "./lib/types";
  let {
    assetBase,
    muted = true,
    subscribe,
    onReady,
    onError,
    editing = false,
    settings,
    onPreview,
    onCommit,
  }: {
    assetBase: string;
    muted?: boolean;
    subscribe: (callback: (event: OverlayEvent) => void) => () => void;
    onReady?: (verify: () => Promise<void>) => void;
    onError?: (error: unknown) => void;
    editing?: boolean;
    settings?: OverlaySettings;
    onPreview?: (settings: OverlaySettings) => void;
    onCommit?: (patch: OverlaySettingsPatch) => Promise<OverlaySettings | void>;
  } = $props();
  let element: HTMLDivElement;
  let stage: ReturnType<typeof createStage> | undefined;
  let draft = $state<OverlaySettings>(
    cloneOverlaySettings(defaultOverlaySettings),
  );
  let canonical = cloneOverlaySettings(defaultOverlaySettings);
  let previousSettings: OverlaySettings | undefined;
  let viewport = $state({ width: 0, height: 0 });
  let dimensions = $state(structuredClone(defaultDimensions));
  let revision = 0;
  let previewing = false;
  function preview(next: OverlaySettings) {
    revision++;
    previewing = true;
    draft = next;
    stage?.applySettings(next);
    onPreview?.(next);
  }
  async function commit(patch: OverlaySettingsPatch) {
    const started = revision;
    try {
      const saved = await onCommit?.(patch);
      canonical = cloneOverlaySettings(
        saved ?? applyOverlayPatch(canonical, patch),
      );
      if (started === revision) {
        previewing = false;
        draft = canonical;
        stage?.applySettings(draft);
      }
      return canonical;
    } catch (error) {
      if (started === revision) {
        previewing = false;
        draft = canonical;
        stage?.applySettings(draft);
      }
      throw error;
    }
  }
  onMount(() => {
    stage = createStage(element, assetBase, muted, editing);
    stage.applySettings(draft);
    void stage.ready
      .then(() => {
        dimensions = stage!.getDimensions();
        onReady?.(stage!.verifyRenderedFrame);
      })
      .catch((error) => onError?.(error));
    const unsubscribe = subscribe((event) => {
      if (event.type === "overlay_settings") {
        canonical = cloneOverlaySettings(event.settings);
        if (previewing) return;
        draft = canonical;
      }
      void stage
        ?.consume(event)
        .then(() => {
          if (stage) dimensions = stage.getDimensions();
        })
        .catch((error) => onError?.(error));
    });
    const resize = new ResizeObserver(() => {
      viewport = { width: element.clientWidth, height: element.clientHeight };
      if (stage) dimensions = stage.getDimensions();
    });
    resize.observe(element);
    return () => {
      resize.disconnect();
      unsubscribe();
      stage?.dispose();
    };
  });
  $effect(() => {
    stage?.setMuted(muted);
  });
  $effect(() => {
    stage?.setEditing(editing);
  });
  $effect(() => {
    if (settings && settings !== previousSettings) {
      previousSettings = settings;
      if (!previewing) {
        canonical = cloneOverlaySettings(settings);
        draft = canonical;
        stage?.applySettings(draft);
      }
    }
  });
</script>

<div class="stage" bind:this={element}>
  {#if editing && onCommit}<OverlayEditor
      settings={draft}
      {viewport}
      {dimensions}
      onPreview={preview}
      onCommit={commit}
      onCancel={() => {
        previewing = false;
        draft = cloneOverlaySettings(canonical);
        stage?.applySettings(draft);
        onPreview?.(draft);
      }}
    />{/if}
</div>

<style>
  .stage {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
  :global(.overlay-presentation) {
    position: absolute;
    top: 5%;
    left: 50%;
    transform: translateX(-50%);
    max-width: 70%;
    max-height: 65%;
    z-index: 4;
    background: #182024f5;
    color: #fff7e5;
    border: 2px solid #d0b365;
    border-radius: 15px;
    padding: 16px 22px;
    overflow: hidden;
    text-align: center;
    box-shadow: 0 7px 25px #0005;
  }
  :global(.overlay-presentation[hidden]) {
    display: none;
  }
  :global(.overlay-presentation h2) {
    font: 650 clamp(14px, 2vw, 25px)/1.3 system-ui;
    margin: 0 0 14px;
  }
  :global(.overlay-presentation img) {
    display: block;
    width: auto;
    max-width: 100%;
    max-height: 50vh;
    object-fit: contain;
    margin: auto;
    border-radius: 8px;
  }
  :global(.overlay-presentation p) {
    font: 450 clamp(12px, 1.8vw, 21px)/1.6 system-ui;
    white-space: pre-wrap;
    text-align: left;
    margin: 0;
  }
</style>
