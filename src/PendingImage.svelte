<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  let { id, name }: { id: string; name: string } = $props();
  let source = $state("");
  let failed = $state(false);
  onMount(() => {
    let disposed = false;
    void invoke<string>("preview_submission", { id }).then(
      (value) => {
        if (!disposed) source = value;
      },
      () => {
        if (!disposed) failed = true;
      },
    );
    return () => {
      disposed = true;
    };
  });
</script>

{#if source}
  <img src={source} alt={`Submitted puppet for ${name}`} />
{:else}
  <span class="preview-placeholder"
    >{failed ? "Image unavailable" : "Loading image…"}</span
  >
{/if}

<style>
  img {
    width: 80px;
    height: 80px;
    flex-shrink: 0;
    object-fit: contain;
    border-radius: 9px;
    background: #111a20;
  }
  .preview-placeholder {
    width: 80px;
    min-height: 80px;
    display: grid;
    place-content: center;
    font-size: 11px;
    text-align: center;
    color: #b3b7ac;
  }
</style>
