<script lang="ts">
  import {
    applyOverlayPatch,
    defaultOverlaySettings,
    subjectLabels,
    type OverlaySettings,
    type OverlaySettingsPatch,
    type OverlaySubject,
  } from "./lib/overlay";
  import { chatBubbleStyleSeeds } from "./renderer/bubbleStyles";
  let {
    settings,
    subject,
    onPreview,
    onCommit,
  }: {
    settings: OverlaySettings;
    subject: OverlaySubject;
    onPreview?: (settings: OverlaySettings) => void;
    onCommit: (patch: OverlaySettingsPatch) => Promise<OverlaySettings | void>;
  } = $props();
  let saving = $state(false);
  let error = $state("");
  const layout = $derived(settings[subject]);
  const isPuppet = $derived(subject === "puppet");
  const isStreamer = $derived(subject === "streamerVoiceBubble");
  const visible = $derived(
    subject === "bumblebee"
      ? settings.bumblebee.visible
      : settings[subject].enabled,
  );
  const bubblesEnabled = $derived(
    isStreamer
      ? true
      : settings[subject as "bumblebee" | "puppet"].chatBubblesEnabled,
  );
  const textSize = $derived(
    isStreamer
      ? settings.streamerVoiceBubble.textSizePercentage
      : settings[subject as "bumblebee" | "puppet"]
          .chatBubbleTextSizePercentage,
  );
  function patch(value: Record<string, unknown>): OverlaySettingsPatch {
    return { [subject]: value } as OverlaySettingsPatch;
  }
  function preview(value: Record<string, unknown>) {
    onPreview?.(applyOverlayPatch(settings, patch(value)));
  }
  async function commit(value: Record<string, unknown>) {
    saving = true;
    error = "";
    try {
      await onCommit(patch(value));
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }
  const number = (event: Event) =>
    Number((event.currentTarget as HTMLInputElement).value);
  const checked = (event: Event) =>
    (event.currentTarget as HTMLInputElement).checked;
</script>

<div class="overlay-controls" aria-label={`${subjectLabels[subject]} settings`}>
  <label class="check"
    ><input
      type="checkbox"
      checked={visible}
      onchange={(e) =>
        commit({
          [subject === "bumblebee" ? "visible" : "enabled"]: checked(e),
        })}
    />
    Show {subjectLabels[subject]}</label
  >
  <fieldset disabled={!visible}>
    <legend>Position</legend>
    <div class="fields">
      <label
        >Horizontal (%)<input
          aria-label={`${subjectLabels[subject]} horizontal position`}
          type="number"
          min="0"
          max="100"
          step="0.1"
          value={layout.position.horizontalPercent}
          onchange={(e) =>
            commit({ position: { horizontalPercent: number(e) } })}
        /></label
      >
      {#if !isPuppet}<label
          >Vertical (%)<input
            aria-label={`${subjectLabels[subject]} vertical position`}
            type="number"
            min="0"
            max="100"
            step="0.1"
            value={(layout.position as { verticalPercent: number })
              .verticalPercent}
            onchange={(e) =>
              commit({ position: { verticalPercent: number(e) } })}
          /></label
        >
      {:else}<label
          >Below screen (%)<input
            type="number"
            min="10"
            max="50"
            step="1"
            value={settings[subject as "puppet"].occlusionPercentage * 100}
            onchange={(e) => commit({ occlusionPercentage: number(e) / 100 })}
          /></label
        >{/if}
    </div>
    <button
      class="reset"
      onclick={() =>
        commit({
          position: defaultOverlaySettings[subject].position,
          anchor: defaultOverlaySettings[subject].anchor,
          ...(isPuppet ? { occlusionPercentage: 0.3 } : {}),
        })}>Reset position</button
    >
    {#if !isStreamer}<label
        >Size <span
          >{Math.round(
            settings[subject as "bumblebee" | "puppet"].scalePercentage * 100,
          )}%</span
        ><input
          aria-label={`${subjectLabels[subject]} size`}
          type="range"
          min="0.05"
          max="1"
          step="0.01"
          value={settings[subject as "bumblebee" | "puppet"].scalePercentage}
          oninput={(e) => preview({ scalePercentage: number(e) })}
          onchange={(e) => commit({ scalePercentage: number(e) })}
        /></label
      ><button
        class="reset"
        onclick={() =>
          commit({
            scalePercentage:
              defaultOverlaySettings[subject as "bumblebee" | "puppet"]
                .scalePercentage,
          })}>Reset size</button
      >{/if}
    {#if isPuppet}
      <label class="check"
        ><input
          type="checkbox"
          checked={settings[subject as "puppet"].showWhenIdle}
          onchange={(e) => commit({ showWhenIdle: checked(e) })}
        /> Show when idle</label
      >
      <label class="check"
        ><input
          type="checkbox"
          checked={settings[subject as "puppet"].nameplatesEnabled}
          onchange={(e) => commit({ nameplatesEnabled: checked(e) })}
        /> Show nameplates</label
      >
    {/if}
  </fieldset>
  <fieldset disabled={!visible}>
    <legend>{isStreamer ? "Bubble" : "Speech bubbles"}</legend>
    {#if !isStreamer}<label class="check"
        ><input
          type="checkbox"
          checked={bubblesEnabled}
          onchange={(e) => commit({ chatBubblesEnabled: checked(e) })}
        /> Show speech bubbles</label
      >{/if}
    <div class:disabled={!bubblesEnabled}>
      <label
        >Style<select
          disabled={!bubblesEnabled}
          value={layout.chatBubbleStyleId}
          onchange={(e) => commit({ chatBubbleStyleId: e.currentTarget.value })}
          >{#each chatBubbleStyleSeeds as style}<option value={style.id}
              >{style.name}</option
            >{/each}</select
        ></label
      >
      <label
        >Text size <span>{Math.round(textSize * 200)}%</span><input
          disabled={!bubblesEnabled}
          aria-label={`${subjectLabels[subject]} bubble text size`}
          type="range"
          min="0.05"
          max="1"
          step="0.025"
          value={textSize}
          oninput={(e) =>
            preview({
              [isStreamer
                ? "textSizePercentage"
                : "chatBubbleTextSizePercentage"]: number(e),
            })}
          onchange={(e) =>
            commit({
              [isStreamer
                ? "textSizePercentage"
                : "chatBubbleTextSizePercentage"]: number(e),
            })}
        /></label
      >
      <div class="fields">
        <label
          >Maximum width (%)<input
            disabled={!bubblesEnabled}
            type="number"
            min="15"
            max="70"
            step="1"
            value={layout.chatBubbleMaxWidthPercent}
            onchange={(e) => commit({ chatBubbleMaxWidthPercent: number(e) })}
          /></label
        ><label
          >Maximum height (%)<input
            disabled={!bubblesEnabled}
            type="number"
            min="10"
            max="70"
            step="1"
            value={layout.chatBubbleMaxHeightPercent}
            onchange={(e) => commit({ chatBubbleMaxHeightPercent: number(e) })}
          /></label
        >
      </div>
    </div>
    {#if isStreamer}<label
        >Tail target radius<input
          type="number"
          min="8"
          max="600"
          value={settings.streamerVoiceBubble.subjectRadiusPx}
          onchange={(e) => commit({ subjectRadiusPx: number(e) })}
        /></label
      ><button
        class="reset"
        onclick={() =>
          commit({
            tail: defaultOverlaySettings.streamerVoiceBubble.tail,
            subjectRadiusPx: 80,
          })}>Reset tail</button
      >{/if}
  </fieldset>
  {#if error}<p role="alert">{error}</p>{/if}
  <div aria-live="polite" class="save-state">
    {saving ? "Saving…" : "Changes save automatically"}
  </div>
</div>

<style>
  .overlay-controls {
    font: 500 13px/1.4 system-ui;
    color: var(--text, #e9ecec);
    min-width: 240px;
  }
  fieldset {
    border: 0;
    border-top: 1px solid var(--border, #293439);
    margin: 16px 0 0;
    padding: 14px 0 0;
    min-width: 0;
  }
  legend {
    font-weight: 750;
    padding-right: 10px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  label {
    display: block;
    margin: 10px 0;
    color: inherit;
    font-size: 12px;
  }
  label > span {
    float: right;
    font-weight: 700;
  }
  input:not([type="checkbox"]),
  select {
    display: block;
    width: 100%;
    box-sizing: border-box;
    margin-top: 5px;
    padding: 7px 9px;
    border: 1px solid #3c484d;
    background: #141c20;
    color: #eef0ec;
    border-radius: 7px;
    font: inherit;
  }
  input[type="range"] {
    padding: 0;
    accent-color: #e4b436;
  }
  input[type="checkbox"] {
    accent-color: #d6a324;
    width: 16px;
    height: 16px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 650;
  }
  .fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .reset {
    border: 0;
    background: transparent;
    color: var(--muted, #95a2a5);
    font-size: 11px;
    padding: 1px 0;
    cursor: pointer;
    text-decoration: underline;
  }
  .disabled,
  fieldset:disabled {
    opacity: 0.5;
  }
  .save-state {
    min-height: 18px;
    font-size: 11px;
    margin-top: 14px;
    color: var(--muted, #95a2a5);
  }
  [role="alert"] {
    color: #ffb0a6;
  }
</style>
