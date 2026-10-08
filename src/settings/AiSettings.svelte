<script lang="ts">
  import type { Settings } from "../lib/types";
  let {
    settings,
    busy,
    models,
    onLoadModels,
  }: {
    settings: Settings;
    busy: boolean;
    models: string[];
    onLoadModels: () => Promise<void>;
  } = $props();
  const id = $props.id();
  const efforts = [
    "default",
    "none",
    "minimal",
    "low",
    "medium",
    "high",
    "xhigh",
  ];
  const grants = [
    {
      id: "discord_resources",
      label: "Discord channels and messages",
      description:
        "Manage channels, send messages and other Discord resources.",
    },
    {
      id: "discord_moderation",
      label: "Discord moderation",
      description: "Manage roles, moderation actions and voice participants.",
    },
    {
      id: "twitch_broadcast",
      label: "Twitch broadcast controls",
      description:
        "Update stream details, create clips, run ads and start raids.",
    },
    {
      id: "twitch_moderation",
      label: "Twitch moderation",
      description: "Moderation actions, chat modes and shoutouts.",
    },
    {
      id: "twitch_polls",
      label: "Twitch polls",
      description: "Create and manage Twitch polls.",
    },
    {
      id: "youtube_moderation",
      label: "YouTube moderation",
      description: "Moderate messages and participants in live chat.",
    },
    {
      id: "youtube_polls",
      label: "YouTube polls",
      description: "Create and manage YouTube live-chat polls.",
    },
  ];
</script>

<div class="ai-settings">
  <p class="intro">
    Choose how Bumblebee thinks and which tools are available. Your provider
    keys stay in the system keyring.
  </p>
  <fieldset disabled={busy}>
    <legend>Bumblebee AI</legend>
    <label class="toggle"
      ><span
        >Enable AI<small
          >Allow eligible chat and voice requests to reach the agent.</small
        ></span
      ><input type="checkbox" bind:checked={settings.aiEnabled} /></label
    >
    <label
      >Chat model<input
        list={`${id}-models`}
        bind:value={settings.openaiModel}
        placeholder="Choose or enter a model ID"
      /></label
    >
    <label
      >Chat reasoning<select bind:value={settings.openaiReasoningEffort}
        >{#each efforts as effort}<option value={effort}
            >{effort === "default"
              ? "Model default"
              : effort === "xhigh"
                ? "Extra high"
                : effort[0].toUpperCase() + effort.slice(1)}</option
          >{/each}</select
      ></label
    >
    <label
      >Voice model<input
        list={`${id}-models`}
        bind:value={settings.openaiVoiceModel}
        placeholder="Use the chat model"
      /></label
    >
    <label
      >Voice reasoning<select bind:value={settings.openaiVoiceReasoningEffort}
        >{#each efforts as effort}<option value={effort}
            >{effort === "default"
              ? "Model default"
              : effort === "xhigh"
                ? "Extra high"
                : effort[0].toUpperCase() + effort.slice(1)}</option
          >{/each}</select
      ></label
    >
    <p>
      Model default uses the provider's usual reasoning. Other levels must be
      supported by the chosen model.
    </p>
    <button type="button" onclick={() => void onLoadModels()}
      >Refresh available models</button
    >
    <datalist id={`${id}-models`}
      >{#each models as model}<option value={model}></option>{/each}</datalist
    >
  </fieldset>
  <fieldset disabled={busy}>
    <legend>Voice transcription</legend>
    <p>Choose the speech-to-text model for your Discord voice captions.</p>
    <label
      >Streamer transcription model<input
        list={`${id}-models`}
        bind:value={settings.streamerTranscriptionModel}
        placeholder="whisper-1"
      /></label
    >
    <p>
      These models transcribe voice input. Bumblebee's spoken output uses the
      voice selected in Audio.
    </p>
  </fieldset>
  <fieldset disabled={busy}>
    <legend>Tools</legend>
    <label class="toggle"
      ><span
        >Web search<small
          >Look up information when a request needs current sources.</small
        ></span
      ><input
        type="checkbox"
        bind:checked={settings.aiWebSearchEnabled}
      /></label
    >
    <label class="toggle"
      ><span
        >Code interpreter<small
          >Analyze data and create files in the provider's execution
          environment.</small
        ></span
      ><input
        type="checkbox"
        bind:checked={settings.aiCodeInterpreterEnabled}
      /></label
    >
    <label class="toggle"
      ><span
        >Image generation<small
          >Create images on request. Files stay private unless you choose to
          show them.</small
        ></span
      ><input
        type="checkbox"
        bind:checked={settings.aiImageGenerationEnabled}
      /></label
    >
    <label
      >Image model<input
        list={`${id}-models`}
        bind:value={settings.imageModel}
        disabled={!settings.aiImageGenerationEnabled}
      /></label
    >
    <label class="toggle"
      ><span
        >Memories<small>Let Bumblebee remember and recall information.</small
        ></span
      ><input
        type="checkbox"
        bind:checked={settings.aiMemoriesEnabled}
      /></label
    >
    <label class="toggle"
      ><span
        >Reminders<small
          >Let Bumblebee schedule reminders. The app must be running for
          delivery.</small
        ></span
      ><input
        type="checkbox"
        bind:checked={settings.aiRemindersEnabled}
      /></label
    >
  </fieldset>
  <fieldset disabled={busy}>
    <legend>Platform actions</legend>
    <p>
      Allow these tool groups explicitly. Provider permissions and action
      confirmations still apply. After changing Twitch grants, reconnect Twitch
      to update its permissions.
    </p>
    {#each grants as grant}<label class="toggle"
        ><span>{grant.label}<small>{grant.description}</small></span><input
          type="checkbox"
          checked={settings.enabledToolGroups.includes(grant.id)}
          onchange={(event) => {
            settings.enabledToolGroups = event.currentTarget.checked
              ? [...new Set([...settings.enabledToolGroups, grant.id])]
              : settings.enabledToolGroups.filter(
                  (value) => value !== grant.id,
                );
          }}
        /></label
      >{/each}
  </fieldset>
</div>

<style>
  .intro,
  p {
    color: var(--muted);
    font-size: 12px;
    line-height: 1.6;
  }
  .intro {
    margin: 0 0 20px;
  }
  fieldset {
    margin: 0 0 22px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: 14px;
    min-width: 0;
  }
  legend {
    padding: 0 6px;
    color: var(--honey);
    font-size: 13px;
    font-weight: 650;
  }
  label {
    font-size: 12px;
  }
  input:not([type="checkbox"]),
  select {
    width: 100%;
    min-width: 0;
  }
  .toggle {
    display: flex;
    flex-direction: row;
    justify-content: space-between;
    gap: 14px;
    align-items: center;
    margin: 10px 0 18px;
  }
  .toggle span {
    color: #e9ecec;
    font-weight: 550;
  }
  small {
    display: block;
    margin-top: 5px;
    color: var(--muted);
    font-weight: 400;
    line-height: 1.55;
  }
  input[type="checkbox"] {
    width: 17px;
    height: 17px;
    flex-shrink: 0;
    accent-color: var(--honey);
  }
</style>
