<script lang="ts">
  import { onMount } from "svelte";
  import { createStage } from "./renderer/stage";
  import type { OverlayEvent } from "./lib/types";
  let {
    assetBase,
    muted = true,
    subscribe,
    onReady,
    onError,
  }: {
    assetBase: string;
    muted?: boolean;
    subscribe: (callback: (event: OverlayEvent) => void) => () => void;
    onReady?: () => void;
    onError?: (error: unknown) => void;
  } = $props();
  let element: HTMLDivElement;
  let stage: ReturnType<typeof createStage> | undefined;
  onMount(() => {
    stage = createStage(element, assetBase, muted);
    void stage.ready.then(() => onReady?.()).catch((error) => onError?.(error));
    const unsubscribe = subscribe((event) => {
      void stage?.consume(event);
    });
    return () => {
      unsubscribe();
      stage?.dispose();
    };
  });
  $effect(() => {
    stage?.setMuted(muted);
  });
</script>

<div class="stage" bind:this={element}></div>

<style>
  .stage {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
  }
  :global(.puppet-label) {
    position: absolute;
    z-index: 3;
    pointer-events: none;
    transform-origin: center;
    background: #171e29e8;
    border: 1px solid #f5df9390;
    border-radius: 8px;
    color: #fff4d0;
    padding: 4px 9px;
    font: 600 13px system-ui;
    white-space: nowrap;
  }
  :global(.speech-bubble) {
    position: absolute;
    bottom: 36%;
    transform: translateX(-50%);
    max-width: 50%;
    z-index: 4;
    color: #202029;
    background: #fff9e8;
    border: 3px solid #433d36;
    border-radius: 25px;
    padding: 14px 20px;
    font: 600 clamp(14px, 2vw, 25px)/1.4 system-ui;
    box-shadow: 0 5px 0 #20202925;
    text-wrap: balance;
    text-align: center;
  }
  :global(.speech-bubble[hidden]) {
    display: none;
  }
  :global(.current-word) {
    color: #9b5304;
    background: #ffe08a;
    border-radius: 3px;
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
