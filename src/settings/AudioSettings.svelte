<script lang="ts">
  import type { Settings, Voice } from "../lib/types";
  let {
    settings,
    voices,
    busy,
    onPreview,
  }: {
    settings: Settings;
    voices: Voice[];
    busy: boolean;
    onPreview: () => Promise<void>;
  } = $props();
  type VolumeKey =
    | "masterVolume"
    | "bumblebeeTtsVolume"
    | "puppetTtsVolume"
    | "wakeChirpVolume"
    | "thinkingSoundVolume"
    | "chatTtsWaitingToneVolume";
  const levels: { key: VolumeKey; label: string; description: string }[] = [
    {
      key: "masterVolume",
      label: "Master volume",
      description: "Overall level for Bumblebee and chat puppets.",
    },
    {
      key: "bumblebeeTtsVolume",
      label: "Bumblebee voice",
      description: "Bumblebee's spoken replies.",
    },
    {
      key: "puppetTtsVolume",
      label: "Chat puppet voices",
      description: "Viewer messages read aloud.",
    },
    {
      key: "wakeChirpVolume",
      label: "Wake chime",
      description: "The sound acknowledging a voice mention.",
    },
    {
      key: "thinkingSoundVolume",
      label: "Thinking sound",
      description: "The sound while Bumblebee prepares a reply.",
    },
    {
      key: "chatTtsWaitingToneVolume",
      label: "Waiting tone",
      description: "The sound while chat speech waits.",
    },
  ];
  let previousVolume: Partial<Record<VolumeKey, number>> = $state({});
  const botVoices = $derived(
    voices.filter((voice) => voice.role === "bumblebee"),
  );
  function toggleMute(key: VolumeKey, event: MouseEvent) {
    if (settings[key] > 0) {
      previousVolume[key] = settings[key];
      settings[key] = 0;
    } else settings[key] = previousVolume[key] ?? 1;
    (event.currentTarget as HTMLElement).dispatchEvent(
      new Event("change", { bubbles: true }),
    );
  }
  const introStops = [
    0, 5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240, 300, 420, 600,
  ];
  const patienceStops = [
    0, 250, 450, 500, 750, 1000, 1500, 2000, 3000, 5000, 10000,
  ];
  const expiryStops = [
    1000, 15000, 30000, 45000, 60000, 90000, 120000, 180000, 300000, 600000,
  ];
  const seconds = (value: number) => {
    const amount = value < 60 ? value : value / 60;
    const unit = value < 60 ? "second" : "minute";
    return `${amount} ${unit}${amount === 1 ? "" : "s"}`;
  };
</script>

<div class="audio-settings">
  <p class="intro">
    Choose where Bumblebee speaks and how chat joins the conversation.
  </p>
  <fieldset disabled={busy}>
    <legend>Audio output</legend>
    <label
      >Play speech through
      <select bind:value={settings.audioOutput}>
        <option value="overlay">OBS overlay</option><option value="discord"
          >Discord voice</option
        >
      </select>
    </label>
    <p>
      Discord output uses your configured voice channel. The desktop preview has
      its own mute control.
    </p>
    <label
      >Bumblebee voice
      <select bind:value={settings.bumblebeeVoice}>
        {#if !botVoices.some((voice) => voice.id === settings.bumblebeeVoice)}<option
            value={settings.bumblebeeVoice}>{settings.bumblebeeVoice}</option
          >{/if}
        {#each botVoices as voice}<option value={voice.id}
            >{voice.name ?? voice.id}</option
          >{/each}
      </select>
    </label>
    <button type="button" onclick={() => void onPreview()}>Preview voice</button
    >
  </fieldset>

  <fieldset disabled={busy}>
    <legend>Chat readout</legend>
    <label class="toggle"
      ><span
        >Read chat aloud<small
          >Read eligible viewer messages using their selected puppet voices.</small
        ></span
      ><input type="checkbox" bind:checked={settings.readChat} /></label
    >
    <label class="toggle"
      ><span
        >Dictate chat AI interactions<small
          >Read chat requests addressed to Bumblebee aloud.</small
        ></span
      ><input
        type="checkbox"
        bind:checked={settings.chatAiDictationEnabled}
      /></label
    >
    <label
      >Speaker introduction cooldown
      <select bind:value={settings.chatTtsSpeakerIntroCooldownSeconds}>
        {#each [...new Set( [...introStops, settings.chatTtsSpeakerIntroCooldownSeconds] )].sort((a, b) => a - b) as value}<option
            {value}>{value === 0 ? "Every message" : seconds(value)}</option
          >{/each}
      </select>
    </label>
    <p>How long before repeating the same chatter's name.</p>
    <label
      >Interrupt patience
      <select bind:value={settings.chatTtsInterruptSilenceMs}>
        {#each [...new Set( [...patienceStops, settings.chatTtsInterruptSilenceMs] )].sort((a, b) => a - b) as value}<option
            {value}
            >{value === 0
              ? "Immediate"
              : value < 1000
                ? `${value} ms`
                : seconds(value / 1000)}</option
          >{/each}
      </select>
    </label>
    <p>
      Wait for this much silence in Discord voice before starting queued chat
      speech.
    </p>
    <label
      >Queued speech expires after
      <select bind:value={settings.chatTtsQueueExpirationMs}>
        {#each [...new Set( [...expiryStops, settings.chatTtsQueueExpirationMs] )].sort((a, b) => a - b) as value}<option
            {value}>{seconds(value / 1000)}</option
          >{/each}
      </select>
    </label>
    <label class="toggle"
      ><span
        >Waiting tone<small>Play a quiet tone while speech is waiting.</small
        ></span
      ><input
        type="checkbox"
        bind:checked={settings.chatTtsWaitingToneEnabled}
      /></label
    >
    <label
      >Blocked words
      <textarea
        rows="3"
        value={settings.chatTtsBlockedWords.join("\n")}
        onchange={(event) => {
          settings.chatTtsBlockedWords = [
            ...new Set(
              event.currentTarget.value
                .split(/[\s,]+/)
                .map((word) => word.trim().toLowerCase())
                .filter(Boolean),
            ),
          ];
        }}
        placeholder="One word per line"
      ></textarea>
    </label>
    <p>Messages containing these words are skipped by chat readout.</p>
  </fieldset>

  <fieldset disabled={busy}>
    <legend>Voice mentions</legend>
    <label class="toggle"
      ><span
        >Listen for Bumblebee<small
          >Only listeners allowed in Discord settings can address Bumblebee.</small
        ></span
      ><input
        type="checkbox"
        bind:checked={settings.voiceMentionsEnabled}
      /></label
    >
    <label
      >Wake phrase<select bind:value={settings.wakeWord}
        ><option value="hey_bumblebee">Hey Bumblebee</option><option
          value="bumblebee">Bumblebee</option
        ></select
      ></label
    >
    {#each [["wakeKeywordSensitivity", "Wake sensitivity"], ["stopKeywordSensitivity", "Stop sensitivity"], ["cancelKeywordSensitivity", "Cancel sensitivity"]] as [key, label]}
      <label
        >{label}<select
          bind:value={
            settings[
              key as
                | "wakeKeywordSensitivity"
                | "stopKeywordSensitivity"
                | "cancelKeywordSensitivity"
            ]
          }
          ><option value="strict">Strict · fewer accidental triggers</option
          ><option value="balanced">Balanced</option><option value="loose"
            >Loose · easier to trigger</option
          ></select
        ></label
      >
    {/each}
    <label class="toggle"
      ><span
        >Replay buffer<small
          >Keep a short voice buffer for replay requests from permitted
          listeners.</small
        ></span
      ><input type="checkbox" bind:checked={settings.replayEnabled} /></label
    >
    <label
      >Replay duration (seconds)<input
        type="number"
        min="5"
        max="120"
        step="1"
        bind:value={settings.replaySeconds}
        disabled={!settings.replayEnabled}
      /></label
    >
  </fieldset>

  <fieldset disabled={busy}>
    <legend>Volume mixer</legend>
    <p>100% is the original level. Levels above 100% amplify the sound.</p>
    {#each levels as level}
      <div class="volume-card">
        <div class="volume-heading">
          <strong>{level.label}</strong><output
            >{Math.round(settings[level.key] * 100)}%</output
          >
        </div>
        <p>{level.description}</p>
        <div class="volume-controls">
          <button
            type="button"
            class="mute"
            aria-label={`${settings[level.key] === 0 ? "Unmute" : "Mute"} ${level.label}`}
            aria-pressed={settings[level.key] === 0}
            onclick={(event) => toggleMute(level.key, event)}
            >{settings[level.key] === 0 ? "Unmute" : "Mute"}</button
          >
          <input
            type="range"
            min="0"
            max="2"
            step="0.05"
            bind:value={settings[level.key]}
            aria-label={level.key === "masterVolume"
              ? "Master volume"
              : `${level.label} volume`}
            aria-valuetext={`${Math.round(settings[level.key] * 100)} percent`}
          />
        </div>
      </div>
    {/each}
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
  select,
  input[type="number"],
  textarea {
    width: 100%;
    min-width: 0;
  }
  textarea {
    resize: vertical;
    border: 1px solid #39484c;
    background: #10191d;
    color: #e9ecec;
    border-radius: 8px;
    padding: 10px;
    font: inherit;
  }
  .volume-card {
    padding: 14px 0;
    border-top: 1px solid var(--border);
  }
  .volume-card:first-of-type {
    border-top: 0;
  }
  .volume-card p {
    margin: 6px 0 10px;
    font-size: 11px;
  }
  .volume-heading {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    font-size: 12px;
  }
  output {
    color: var(--green);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  .volume-controls {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .mute {
    width: 60px;
    padding: 7px 6px;
    font-size: 10px;
    border-radius: 999px;
  }
  input[type="range"] {
    flex: 1;
    width: 100%;
    min-width: 0;
    padding: 0;
    accent-color: var(--green);
  }
</style>
