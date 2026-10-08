<script lang="ts">
  import { onMount } from "svelte";
  import VoiceCaptionSettings from "./VoiceCaptionSettings.svelte";
  import type { Settings, DiscordOption, AudiencePolicy } from "../lib/types";
  type Platform = "discord" | "twitch" | "youtube";
  let {
    platform,
    settings,
    saved,
    busy,
    invoke,
    run,
    onSaveSecret,
    onAuthorize,
    onValidate,
  }: {
    platform: Platform;
    settings: Settings;
    saved: Record<string, boolean>;
    busy: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
    run: (label: string, action: () => Promise<unknown>) => Promise<void>;
    onSaveSecret: (name: string, value: string) => Promise<void>;
    onAuthorize: (provider: string) => Promise<void>;
    onValidate: (provider: string) => Promise<void>;
  } = $props();
  let secret = $state("");
  let guilds = $state<DiscordOption[]>([]);
  let channels = $state<DiscordOption[]>([]);
  let roles = $state<DiscordOption[]>([]);
  let loading = $state(false);
  let discoveryError = $state("");
  let discoveryGeneration = 0;
  const policy = $derived(settings.chatPlatforms[platform]);
  async function loadChannels(guildId: string) {
    const generation = ++discoveryGeneration;
    channels = [];
    roles = [];
    if (!guildId) return;
    loading = true;
    discoveryError = "";
    try {
      const next = await invoke<{
        channels: DiscordOption[];
        roles: DiscordOption[];
      }>("discord_options", { guildId });
      if (
        generation !== discoveryGeneration ||
        settings.discordGuildId !== guildId
      )
        return;
      channels = next.channels;
      roles = next.roles;
    } catch (e) {
      if (generation === discoveryGeneration) discoveryError = String(e);
    } finally {
      if (generation === discoveryGeneration) loading = false;
    }
  }
  async function discover() {
    loading = true;
    discoveryError = "";
    try {
      guilds = await invoke<DiscordOption[]>("discord_guilds");
      await loadChannels(settings.discordGuildId);
    } catch (e) {
      discoveryError = String(e);
    } finally {
      loading = false;
    }
  }
  function toggleRole(audience: AudiencePolicy, id: string, checked: boolean) {
    audience.roleIds = checked
      ? [...audience.roleIds.filter((v) => v !== id), id]
      : audience.roleIds.filter((v) => v !== id);
  }
  onMount(() => {
    if (platform === "discord" && saved.discord_bot) void discover();
    return () => {
      discoveryGeneration++;
    };
  });
</script>

{#if platform === "discord"}
  <section class="settings-card">
    <h3>Discord connection</h3>
    <label
      >Bot token<input
        type="password"
        autocomplete="off"
        bind:value={secret}
        placeholder={saved.discord_bot
          ? "Saved in keyring"
          : "Your Discord bot token"}
      /></label
    >
    <div class="button-row">
      <button
        disabled={busy || !secret.trim()}
        onclick={() =>
          run("Saving Discord token", async () => {
            await onSaveSecret("discord_bot", secret);
            secret = "";
            await discover();
          })}>Save token</button
      ><button
        disabled={busy}
        onclick={() => run("Validating Discord", () => onValidate("discord"))}
        >Validate</button
      ><button
        disabled={loading || busy || !saved.discord_bot}
        onclick={discover}>{loading ? "Loading…" : "Refresh servers"}</button
      >
    </div>
    {#if discoveryError}<p role="alert" class="discovery-error">
        {discoveryError}
      </p>{/if}
    <label
      >Server<select
        bind:value={settings.discordGuildId}
        onchange={(event) => {
          settings.discordGuildId = event.currentTarget.value;
          settings.discordTextChannelId = "";
          settings.discordVoiceChannelId = "";
          void loadChannels(settings.discordGuildId);
        }}
      >
        <option value="">No server</option>
        {#if settings.discordGuildId && !guilds.some((g) => g.id === settings.discordGuildId)}<option
            value={settings.discordGuildId}
            >Saved server · {settings.discordGuildId}</option
          >{/if}
        {#each guilds as guild}<option value={guild.id}>{guild.name}</option
          >{/each}
      </select></label
    >
    <label
      >Voice channel<select
        bind:value={settings.discordVoiceChannelId}
        disabled={!settings.discordGuildId || loading}
      >
        <option value="">No voice channel</option>
        {#if settings.discordVoiceChannelId && !channels.some((c) => c.id === settings.discordVoiceChannelId)}<option
            value={settings.discordVoiceChannelId}
            >Saved channel · {settings.discordVoiceChannelId}</option
          >{/if}
        {#each channels.filter((c) => c.kind === 2) as channel}<option
            value={channel.id}>◖ {channel.name}</option
          >{/each}
      </select></label
    >
    <label
      >Text channel<select
        bind:value={settings.discordTextChannelId}
        disabled={!settings.discordGuildId || loading}
      >
        <option value="">No text channel</option>
        {#if settings.discordTextChannelId && !channels.some((c) => c.id === settings.discordTextChannelId)}<option
            value={settings.discordTextChannelId}
            >Saved channel · {settings.discordTextChannelId}</option
          >{/if}
        {#each channels.filter( (c) => [0, 2, 5].includes(c.kind) ) as channel}<option
            value={channel.id}
            >{channel.kind === 2 ? "Voice chat" : "#"} {channel.name}</option
          >{/each}
      </select></label
    >
    <label
      >Your Discord user ID<input
        bind:value={settings.ownerDiscordId}
        inputmode="numeric"
      /></label
    >
    <small
      >The selected voice channel’s text chat can be used as the text
      destination.</small
    >
  </section>
{:else if platform === "twitch"}
  <section class="settings-card">
    <h3>Twitch connection</h3>
    <label
      >Public application client ID<input
        bind:value={settings.twitchClientId}
      /></label
    >
    <label
      >Channel login<input
        bind:value={settings.twitchChannel}
        placeholder="your_channel"
      /></label
    >
    <p>Messages are posted as the Twitch account you authorize.</p>
    <button
      disabled={busy}
      onclick={() => run("Authorizing Twitch", () => onAuthorize("twitch"))}
      >{saved.twitch_tokens ? "Reauthorize Twitch" : "Authorize Twitch"}</button
    >
  </section>
{:else}
  <section class="settings-card">
    <h3>YouTube connection</h3>
    <label
      >Desktop application client ID<input
        bind:value={settings.googleClientId}
      /></label
    >
    <label
      >Desktop client secret<input
        type="password"
        autocomplete="off"
        bind:value={secret}
        placeholder={saved.google_client_secret
          ? "Saved in keyring"
          : "Desktop OAuth client secret"}
      /></label
    >
    <div class="button-row">
      <button
        disabled={busy || !secret.trim()}
        onclick={() =>
          run("Saving Google client", async () => {
            await onSaveSecret("google_client_secret", secret);
            secret = "";
          })}>Save secret</button
      ><button
        disabled={busy}
        onclick={() => run("Authorizing YouTube", () => onAuthorize("youtube"))}
        >{saved.google_tokens
          ? "Reauthorize YouTube"
          : "Authorize YouTube"}</button
      >
    </div>
    <label
      >Live chat ID<input
        bind:value={settings.youtubeLiveChatId}
        placeholder="Discover my active broadcast"
      /></label
    >
    <small
      >Leave blank to find your single active broadcast when you start a
      session.</small
    >
  </section>
{/if}

<section class="settings-card">
  <h3>Chat connection</h3>
  <label class="check"
    ><input type="checkbox" bind:checked={policy.monitor} />Monitor this
    platform’s chat</label
  >
  <label class="check"
    ><input type="checkbox" bind:checked={policy.relay} />Relay chat to other
    connected platforms</label
  >
  <p>
    Relay destinations must also have relay enabled. Commands and Bumblebee’s
    own echoes are excluded.
  </p>
</section>
{#each [["mentions", "Who can talk to Bumblebee?"], ["readout", "Whose chat is read aloud?"]] as [key, title]}
  {@const audience = policy[key as "mentions" | "readout"]}
  <section class="settings-card">
    <h3>{title}</h3>
    <label class="check"
      ><input type="checkbox" bind:checked={audience.everyone} />Everyone</label
    >
    {#if !audience.everyone}
      {#if platform === "discord"}
        {#each roles as role}<label class="check"
            ><input
              type="checkbox"
              checked={audience.roleIds.includes(role.id)}
              onchange={(e) =>
                toggleRole(audience, role.id, e.currentTarget.checked)}
            />{role.name}</label
          >{/each}
        {#if !roles.length}<label
            >Allowed role IDs<input
              value={audience.roleIds.join(", ")}
              onchange={(e) =>
                (audience.roleIds = e.currentTarget.value
                  .split(/[,\s]+/)
                  .filter(Boolean))}
            /></label
          >{/if}
      {:else}
        {#if platform === "twitch"}<label class="check"
            ><input
              type="checkbox"
              bind:checked={audience.followers}
            />Followers</label
          ><label class="check"
            ><input
              type="checkbox"
              bind:checked={audience.subscribers}
            />Subscribers</label
          ><label class="check"
            ><input type="checkbox" bind:checked={audience.vips} />VIPs</label
          >{:else}<label class="check"
            ><input type="checkbox" bind:checked={audience.members} />Channel
            members</label
          >{/if}
        <label class="check"
          ><input
            type="checkbox"
            bind:checked={audience.moderators}
          />Moderators</label
        >
      {/if}
    {/if}
    <small
      >Your owner account is allowed unless explicitly blocked in Overrides.</small
    >
  </section>
{/each}
{#if platform === "twitch" && (policy.mentions.followers || policy.readout.followers)}<p
  >
    Follower permissions require fresh Twitch authorization with follower-read
    access.
  </p>{/if}

{#if platform === "discord"}<VoiceCaptionSettings {settings} {busy} />{/if}

<style>
  .discovery-error {
    color: #ffbbb0 !important;
  }
  small {
    display: block;
    font-size: 11px;
    color: var(--muted);
    line-height: 1.6;
  }
</style>
