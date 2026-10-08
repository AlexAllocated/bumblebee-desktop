<script lang="ts">
  import type { Settings } from "../lib/types";
  let { settings, busy }: { settings: Settings; busy: boolean } = $props();
</script>

<fieldset class="captions" disabled={busy}>
  <legend>Streamer captions</legend>
  <p>
    Show your spoken Discord conversation in the separate streamer caption
    bubble. Captioning sends your audio to OpenAI for transcription; other
    participants are not captioned.
  </p>
  <label class="toggle"
    ><span
      >Enable streamer captions<small
        >Uses your configured Discord identity. AI and the streamer caption
        bubble must also be enabled.</small
      ></span
    ><input
      type="checkbox"
      bind:checked={settings.voiceCaptionsEnabled}
    /></label
  >
  <p>The streamer transcription model is configured in AI settings.</p>
</fieldset>

<style>
  fieldset {
    min-width: 0;
    margin: 22px 0 0;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: 14px;
  }
  legend {
    padding: 0 6px;
    color: var(--honey);
    font-size: 13px;
    font-weight: 650;
  }
  p,
  small {
    color: var(--muted);
    font-size: 12px;
    line-height: 1.6;
  }
  label {
    font-size: 12px;
  }
  .toggle {
    display: flex;
    flex-direction: row;
    justify-content: space-between;
    gap: 14px;
    align-items: center;
    margin: 14px 0;
  }
  .toggle span {
    font-weight: 550;
  }
  small {
    display: block;
    margin-top: 5px;
    font-weight: 400;
  }
  input[type="checkbox"] {
    width: 17px;
    height: 17px;
    flex-shrink: 0;
    accent-color: var(--honey);
  }
</style>
