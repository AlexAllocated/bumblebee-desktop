<script lang="ts">
  import { untrack } from "svelte";
  import type { Chatter, ChatterOverrides, OverrideChoice } from "../lib/types";
  let {
    chatter,
    busy,
    onSave,
  }: {
    chatter: Chatter;
    busy: boolean;
    onSave: (overrides: ChatterOverrides) => Promise<void>;
  } = $props();
  type PolicyKey = "chatPuppet" | "relay" | "ttsWait" | "aiAccess";
  const policies: { key: PolicyKey; label: string; description: string }[] = [
    {
      key: "chatPuppet",
      label: "Chat puppet readout",
      description: "Whether this chatter's eligible messages are read aloud.",
    },
    {
      key: "relay",
      label: "Chat relay",
      description:
        "Whether this chatter's messages can be relayed to other connected chats.",
    },
    {
      key: "ttsWait",
      label: "Pause chat while speaking",
      description:
        "Whether this person speaking in Discord makes queued chat readout wait.",
    },
    {
      key: "aiAccess",
      label: "AI access",
      description:
        "Whether this chatter may address Bumblebee. Master switches and voice consent still apply.",
    },
  ];
  const choices: { value: OverrideChoice; label: string }[] = [
    { value: "inherit", label: "Use platform setting" },
    { value: "allow", label: "Allow" },
    { value: "block", label: "Block" },
  ];
  let draft = $state<ChatterOverrides>(
    untrack(() => ({ ...chatter.overrides })),
  );
  let sourceKey = untrack(() => `${chatter.platform}:${chatter.userId}`);
  let sourceValue = untrack(() => JSON.stringify(chatter.overrides));
  let saving = $state(false);
  let error = $state("");
  let saved = $state(false);
  const dirty = $derived(
    JSON.stringify(draft) !== JSON.stringify(chatter.overrides),
  );
  $effect(() => {
    const key = `${chatter.platform}:${chatter.userId}`;
    const value = JSON.stringify(chatter.overrides);
    if (key !== sourceKey || value !== sourceValue) {
      draft = { ...chatter.overrides };
      sourceKey = key;
      sourceValue = value;
      error = "";
      saved = false;
    }
  });
  async function save() {
    saving = true;
    error = "";
    saved = false;
    const pending = {
      ...draft,
      textModel: draft.textModel?.trim() || null,
      voiceModel: draft.voiceModel?.trim() || null,
    };
    try {
      await onSave(pending);
      draft = pending;
      saved = true;
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      saving = false;
    }
  }
</script>

<form
  class="overrides"
  onchange={(event) => {
    event.stopPropagation();
    saved = false;
  }}
  onsubmit={(event) => {
    event.preventDefault();
    event.stopPropagation();
    void save();
  }}
>
  <fieldset disabled={busy || saving}>
    <legend>Chatter overrides</legend>
    <p>
      Leave a control on its platform setting to follow the shared rules.
      Overrides apply only to {chatter.displayName}.
    </p>
    {#each policies as policy}
      <label
        >{policy.label}<small>{policy.description}</small>
        <select bind:value={draft[policy.key]}
          >{#each choices as choice}<option value={choice.value}
              >{choice.label}</option
            >{/each}</select
        >
      </label>
    {/each}
    <label
      >Chat model override<input
        value={draft.textModel ?? ""}
        oninput={(event) =>
          (draft.textModel = event.currentTarget.value || null)}
        placeholder="Use the shared chat model"
        maxlength="200"
      /></label
    >
    <label
      >Voice model override<input
        value={draft.voiceModel ?? ""}
        oninput={(event) =>
          (draft.voiceModel = event.currentTarget.value || null)}
        placeholder="Use the shared voice model"
        maxlength="200"
      /></label
    >
    <div class="actions">
      <button type="submit" disabled={!dirty}>Apply overrides</button><button
        type="button"
        disabled={!dirty}
        onclick={() => {
          draft = { ...chatter.overrides };
          error = "";
          saved = false;
        }}>Discard changes</button
      >
    </div>
    {#if saving}<p role="status">Saving overrides…</p>{:else if error}<p
        class="error"
        role="alert"
      >
        {error}
      </p>{:else if saved}<p class="saved" role="status">
        Overrides saved.
      </p>{/if}
  </fieldset>
</form>

<style>
  fieldset {
    min-width: 0;
    margin: 14px 0 0;
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: 12px;
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
    margin: 14px 0;
  }
  small {
    display: block;
    margin: 5px 0;
  }
  input,
  select {
    width: 100%;
    min-width: 0;
  }
  .actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 16px;
  }
  .error {
    color: #ffada6;
  }
  .saved {
    color: var(--green);
  }
</style>
