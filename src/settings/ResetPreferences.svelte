<script module lang="ts">
  export type ResetScope =
    "audio" | "ai" | "discord" | "twitch" | "youtube" | "all";
</script>

<script lang="ts">
  import { tick } from "svelte";
  let {
    scope,
    busy,
    onReset,
  }: {
    scope: ResetScope;
    busy: boolean;
    onReset: (scope: ResetScope) => Promise<void>;
  } = $props();
  let confirming = $state(false),
    resetting = $state(false),
    error = $state(""),
    complete = $state(false);
  let trigger = $state<HTMLButtonElement>();
  let keep = $state<HTMLButtonElement>();
  const titles: Record<ResetScope, string> = {
    audio: "chat and audio",
    ai: "AI",
    discord: "Discord behavior",
    twitch: "Twitch behavior",
    youtube: "YouTube behavior",
    all: "all behavior preferences",
  };
  const descriptions: Record<ResetScope, string> = {
    audio:
      "Restore Bumblebee's voice, audio output, mixer levels, chat readout, blocked words, queue timing, wake settings and replay preferences.",
    ai: "Restore models, reasoning levels and AI tools. Explicit platform action grants will be cleared; saved memories and reminders remain.",
    discord:
      "Restore Discord chat monitoring, relay, audience rules, voice-listening permissions and streamer captions. Server, channels and your Discord identity remain.",
    twitch:
      "Restore Twitch chat monitoring, relay and audience rules. Your connected account and channel remain.",
    youtube:
      "Restore YouTube chat monitoring, relay and audience rules. Your connected account and live-chat destination remain.",
    all: "Restore audio, AI, chat policies, voice permissions, image-submission preferences and the entire overlay layout. Individual chatter profiles and overrides remain.",
  };
  async function request() {
    error = "";
    complete = false;
    confirming = true;
    await tick();
    keep?.focus();
  }
  async function cancel() {
    confirming = false;
    await tick();
    trigger?.focus();
  }
  async function reset() {
    resetting = true;
    error = "";
    try {
      await onReset(scope);
      confirming = false;
      complete = true;
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      resetting = false;
      if (complete) {
        await tick();
        trigger?.focus();
      }
    }
  }
</script>

<div class="reset-preferences" onchange={(event) => event.stopPropagation()}>
  {#if confirming}
    <div class="confirmation">
      <h3>Reset {titles[scope]}?</h3>
      <p>{descriptions[scope]}</p>
      <p>
        Provider credentials, connections, OBS URL and port, startup preference,
        memories, reminders and history are preserved.
      </p>
      <div class="actions">
        <button
          type="button"
          class="confirm"
          disabled={busy || resetting}
          onclick={() => void reset()}
          >{resetting ? "Resetting…" : "Reset preferences"}</button
        ><button
          bind:this={keep}
          type="button"
          disabled={busy || resetting}
          onclick={() => void cancel()}>Keep current settings</button
        >
      </div>
    </div>
  {:else}
    <button
      bind:this={trigger}
      type="button"
      disabled={busy || resetting}
      onclick={() => void request()}>Reset {titles[scope]}</button
    >
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{:else if complete}<p
      role="status"
    >
      Preferences reset.
    </p>{/if}
</div>

<style>
  .reset-preferences {
    margin-top: 24px;
    padding-top: 18px;
    border-top: 1px solid var(--border);
  }
  h3 {
    margin: 0 0 8px;
    font-size: 14px;
  }
  p {
    color: var(--muted);
    font-size: 12px;
    line-height: 1.6;
  }
  .confirmation {
    padding: 14px;
    border: 1px solid #886445;
    border-radius: 10px;
    background: #30231b;
  }
  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .confirm {
    border-color: #c08a5b;
  }
  .error {
    color: #ffada6;
  }
</style>
