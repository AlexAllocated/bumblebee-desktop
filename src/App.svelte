<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, isTauri } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
  import Stage from "./Stage.svelte";
  import PendingImage from "./PendingImage.svelte";
  import type {
    Chatter,
    OverlayEvent,
    Snapshot,
    ProviderStatus,
  } from "./lib/types";
  let snapshot: Snapshot | null = $state(null);
  let drawer = $state(false);
  let section = $state("connections");
  let busy = $state("");
  let error = $state("");
  let notice = $state("");
  let profileSearch = $state("");
  let voiceSearch = $state("");
  let previewText = $state("Hey, welcome to the stream!");
  let previewMuted = $state(true);
  let autostart = $state(false);
  let secretInputs: Record<string, string> = $state({});
  let answers: Record<string, string> = $state({});
  let artifactPreview = $state<string | null>(null);
  let artifactLabel = $state("");
  let log: { time: string; text: string; sender: string }[] = $state([]);
  let statuses: ProviderStatus[] = $state([]);
  const subscribers = new Set<(event: OverlayEvent) => void>();
  function rendererResult(error: unknown = null) {
    if (isTauri())
      void invoke("frontend_result", {
        error: error == null ? null : String(error),
      });
  }
  async function rendererReady(verify: () => Promise<void>) {
    if (!isTauri() || !(await invoke<boolean>("frontend_probe_required")))
      return;
    try {
      await verify();
      rendererResult();
    } catch (error) {
      rendererResult(error);
    }
  }
  const subscribe = (callback: (event: OverlayEvent) => void) => {
    subscribers.add(callback);
    if (snapshot)
      callback({
        type: "overlay_settings",
        settings: snapshot.overlaySettings,
      });
    return () => subscribers.delete(callback);
  };
  const assetBase = $derived.by(() =>
    snapshot ? new URL("./", snapshot.overlayUrl).toString() : "",
  );
  const voices = $derived.by(
    () =>
      snapshot?.voices.filter(
        (v) =>
          !voiceSearch ||
          `${v.name} ${v.voiceName}`
            .toLowerCase()
            .includes(voiceSearch.toLowerCase()),
      ) ?? [],
  );
  async function refresh() {
    const next = await invoke<Snapshot>("get_snapshot");
    if (drawer && snapshot) next.settings = snapshot.settings;
    snapshot = next;
    statuses = snapshot.statuses;
    if (snapshot.credentialStoreError || snapshot.overlayError)
      error = snapshot.credentialStoreError ?? snapshot.overlayError ?? "";
  }
  async function activity() {
    if (!snapshot) return;
    const next =
      await invoke<
        Pick<
          Snapshot,
          | "pendingInputs"
          | "interruptedTurns"
          | "pendingImages"
          | "active"
          | "artifacts"
        >
      >("get_activity");
    snapshot = { ...snapshot, ...next };
  }
  async function run(label: string, action: () => Promise<unknown>) {
    if (busy) return;
    busy = label;
    error = "";
    notice = "";
    try {
      await action();
    } catch (e) {
      error = String(e);
    } finally {
      busy = "";
    }
  }
  async function save() {
    if (snapshot) {
      snapshot.settings = await invoke("save_settings", {
        settings: snapshot.settings,
      });
      snapshot.overlaySettings = await invoke("save_overlay_settings", {
        settings: snapshot.overlaySettings,
      });
      notice = snapshot.active
        ? "Settings saved. Active integrations are reconnecting with the new configuration."
        : "Settings saved.";
      await activity();
    }
  }
  async function saveSecret(name: string) {
    const value = secretInputs[name]?.trim();
    if (!value) return;
    await invoke("set_secret", { name, value });
    secretInputs[name] = "";
    if (snapshot) snapshot.secrets[name] = true;
    await activity();
    notice =
      "Credential saved in your operating-system keyring. Start the session to connect with it.";
  }
  async function validate(provider: string) {
    await save();
    await invoke("validate_provider", { provider });
    await refresh();
    notice = "Configuration validated.";
  }
  async function authorize(provider: string) {
    await save();
    const result = await invoke<{ url: string; userCode?: string }>(
      "authorize_provider",
      { provider },
    );
    if (result.userCode)
      notice = `Enter code ${result.userCode} in your browser.`;
    await activity();
    await openUrl(result.url);
  }
  async function updateChatter(
    chatter: Chatter,
    change: Record<string, unknown>,
  ) {
    await invoke("update_chatter", {
      platform: chatter.platform,
      userId: chatter.userId,
      ...change,
    });
    await refresh();
  }
  function picture(chatter: Chatter) {
    return (
      assetBase +
      (chatter.imageHash
        ? `images/${chatter.imageHash}.png`
        : `puppets/images/${chatter.puppetId}.png`)
    );
  }
  onMount(() => {
    if (!isTauri()) return;
    let disposed = false;
    const unlisten: (() => void)[] = [];
    const activityTimer = setInterval(() => {
      void activity().catch(() => {});
    }, 3000);
    void run("Loading", async () => {
      await refresh();
      autostart = await isEnabled();
    });
    void Promise.all([
      listen<OverlayEvent>("bumblebee:overlay", ({ payload }) => {
        if (payload.type === "overlay_settings" && snapshot)
          snapshot.overlaySettings = payload.settings;
        for (const callback of subscribers) callback(payload);
        if (
          payload.type === "chat" ||
          payload.type === "speech" ||
          payload.type === "status"
        ) {
          const text =
            payload.type === "status" ? payload.message : payload.text;
          const sender =
            payload.type === "status"
              ? "Bumblebee"
              : (payload.chatter?.displayName ?? "Bumblebee");
          log = [
            ...log.slice(-99),
            {
              time: new Date().toLocaleTimeString([], {
                hour: "2-digit",
                minute: "2-digit",
              }),
              text,
              sender,
            },
          ];
        }
      }),
      listen<ProviderStatus>("bumblebee:provider-status", ({ payload }) => {
        statuses = [
          ...statuses.filter((s) => s.provider !== payload.provider),
          payload,
        ];
      }),
      listen("bumblebee:profiles-changed", () => {
        if (!busy) void refresh();
      }),
    ]).then((results) => {
      if (disposed) results.forEach((fn) => fn());
      else unlisten.push(...results);
    });
    return () => {
      disposed = true;
      clearInterval(activityTimer);
      unlisten.forEach((fn) => fn());
    };
  });
</script>

<div class="app-shell">
  <header>
    <div class="brand">
      <img src="./bumblebee.png" alt="" />
      <div>
        <strong>Bumblebee</strong><span
          >YOUR STREAM, WITH A LITTLE MORE BUZZ.</span
        >
      </div>
    </div>
    <div class="header-actions">
      <span class:live={snapshot?.active} class="session-indicator"
        ><i></i>{snapshot?.active
          ? "Session running"
          : "Ready when you are"}</span
      ><button class="quiet" onclick={() => (drawer = true)}
        >⚙ <span>Settings</span></button
      >
    </div>
  </header>

  <main>
    <div class="welcome">
      <div>
        <p class="eyebrow">AT HOME ON YOUR DESKTOP</p>
        <h1>Make yourself at home.</h1>
        <p>Your companion, your voices, your little corner of the internet.</p>
      </div>
      <button
        class="primary"
        disabled={!snapshot || !!busy}
        onclick={() =>
          run("Session", async () => {
            await invoke(snapshot?.active ? "stop_session" : "start_session");
            await refresh();
          })}
        >{snapshot?.active ? "End session" : "Start session"}<span>↗</span
        ></button
      >
    </div>
    {#if error}<div role="alert" class="banner error">
        <strong>Something needs attention</strong><span>{error}</span><button
          onclick={() => (error = "")}
          aria-label="Dismiss error">×</button
        >
      </div>{/if}
    {#if notice}<div role="status" class="banner notice">
        {notice}<button
          onclick={() => (notice = "")}
          aria-label="Dismiss notice">×</button
        >
      </div>{/if}
    {#each snapshot?.pendingInputs ?? [] as pending}<section
        class="panel pending-request"
      >
        <div>
          <p class="eyebrow">
            {pending.kind === "confirmation"
              ? "YOUR APPROVAL IS NEEDED"
              : "BUMBLEBEE HAS A QUESTION"}
          </p>
          <h3>{pending.prompt}</h3>
          <small
            >{pending.actor} · expires {new Date(
              pending.expiresAt,
            ).toLocaleTimeString()}</small
          >
        </div>
        <div class="button-row">
          {#each pending.choices as choice}<button
              disabled={!!busy || !snapshot?.active}
              onclick={() =>
                run("Answering", async () => {
                  await invoke("answer_pending", {
                    id: pending.id,
                    answer: choice,
                  });
                  await activity();
                })}>{choice}</button
            >{:else}<input
              aria-label="Answer Bumblebee"
              bind:value={answers[pending.id]}
              maxlength={4000}
            /><button
              disabled={!!busy ||
                !snapshot?.active ||
                !answers[pending.id]?.trim()}
              onclick={() =>
                run("Answering", async () => {
                  await invoke("answer_pending", {
                    id: pending.id,
                    answer: answers[pending.id],
                  });
                  await activity();
                })}>Send answer</button
            >{/each}
        </div>
      </section>{/each}
    {#each snapshot?.interruptedTurns ?? [] as turn}<section
        class="panel pending-request"
      >
        <div>
          <p class="eyebrow">INTERRUPTED ACTION</p>
          <p>
            A turn from {turn.actor} stopped before its outcome was known. Check the
            affected service before making a new request. Bumblebee will not repeat
            it automatically.
          </p>
        </div>
        <button
          disabled={!!busy}
          onclick={() =>
            run("Acknowledging", async () => {
              await invoke("dismiss_interrupted", { id: turn.id });
              await activity();
            })}>Acknowledge</button
        >
      </section>{/each}
    <div class="dashboard-grid">
      <section class="preview panel">
        <div class="panel-heading">
          <div>
            <span class="status-dot"></span><strong>Backstage</strong><small
              >Desktop preview</small
            >
          </div>
          <button class="subtle" onclick={() => (previewMuted = !previewMuted)}
            >{previewMuted ? "Unmute preview" : "Mute preview"}</button
          >
        </div>
        <div class="preview-stage">
          {#if snapshot}{#key assetBase}<Stage
                {assetBase}
                {subscribe}
                muted={previewMuted}
                onReady={rendererReady}
                onError={rendererResult}
              />{/key}{:else}<div class="unavailable">
              <img src="./bumblebee.png" alt="Bumblebee" />
              <p>Open Bumblebee as a desktop app to connect your stream.</p>
            </div>{/if}<span class="preview-label"
            >1920 × 1080 · transparent in OBS</span
          >
        </div>
        <form
          class="preview-input"
          onsubmit={(e) => {
            e.preventDefault();
            void run("Speech", () =>
              invoke("preview_speech", { text: previewText }),
            );
          }}
        >
          <input
            aria-label="Speech preview text"
            bind:value={previewText}
            maxlength={500}
            placeholder="Give Bumblebee something to say…"
          /><button
            disabled={!snapshot || !!busy}
            class="primary compact"
            type="submit">Say it</button
          ><button
            disabled={!snapshot}
            type="button"
            class="quiet compact"
            onclick={() => run("Cancel", () => invoke("cancel_speech"))}
            aria-label="Stop speech">■</button
          >
        </form>
      </section>
      <aside class="right-column">
        <section class="panel obs">
          <div class="eyebrow">BRING BUMBLEBEE ON STREAM</div>
          <h2>A window into your world.</h2>
          <p>
            Add a Browser Source in OBS, paste your overlay URL, and set its
            size to 1920 × 1080.
          </p>
          <button
            class="secondary"
            disabled={!snapshot || !!snapshot.overlayError}
            onclick={() =>
              run("Copying", async () => {
                await navigator.clipboard.writeText(snapshot!.overlayUrl);
                notice =
                  "Overlay URL copied. Paste it into an OBS Browser Source.";
              })}>Copy OBS overlay URL <span>↗</span></button
          ><small
            >The overlay stays on this computer. Keep Bumblebee running while
            you stream.</small
          >
        </section>
        <section class="panel connections">
          <div class="panel-heading">
            <strong>Connections</strong><button
              class="subtle"
              onclick={() => {
                section = "connections";
                drawer = true;
              }}>Manage</button
            >
          </div>
          {#each ["twitch", "youtube", "discord", "azure_speech", "openai"] as provider}{@const status =
              statuses.find((s) => s.provider === provider)}
            <div class="connection-row">
              <span
                class="provider-icon"
                class:connected={status?.state === "connected"}
                >{provider === "azure_speech"
                  ? "S"
                  : provider.slice(0, 1).toUpperCase()}</span
              ><span
                >{provider === "azure_speech"
                  ? "Azure Speech"
                  : provider === "openai"
                    ? "OpenAI"
                    : provider.slice(0, 1).toUpperCase() +
                      provider.slice(1)}</span
              ><small title={status?.message}
                >{status?.state ?? "Not configured"}</small
              >
            </div>{/each}
        </section>
      </aside>
      <section class="panel activity">
        <div class="panel-heading">
          <strong>Conversation</strong><small
            >{log.length ? "This session" : "The stage is yours"}</small
          >
        </div>
        <div class="activity-log" aria-live="polite">
          {#each log as line}<div class="log-line">
              <time>{line.time}</time><strong>{line.sender}</strong><span
                >{line.text}</span
              >
            </div>{:else}<div class="empty">
              <span>✦</span>
              <p>Every chatter has a voice.</p>
              <small
                >Connect a platform and start a session. Your conversation will
                appear here.</small
              >
            </div>{/each}
        </div>
      </section>
    </div>
    {#if snapshot?.artifacts.length}<section class="panel generated-files">
        <div class="panel-heading">
          <strong>Made with Bumblebee</strong><button
            class="subtle"
            onclick={() =>
              run("Hiding presentation", () => invoke("hide_artifact"))}
            >Hide stream presentation</button
          >
        </div>
        <p>
          Generated files stay on this computer until you choose to show them on
          stream.
        </p>
        {#each snapshot.artifacts.slice(-20).reverse() as artifact}<div
            class="artifact-row"
          >
            <div>
              <strong>{artifact.label}</strong><small>{artifact.filename}</small
              >
            </div>
            <div class="button-row">
              {#if ["image/png", "image/jpeg", "image/webp"].includes(artifact.mediaType)}<button
                  disabled={!!busy}
                  onclick={() =>
                    run("Previewing image", async () => {
                      artifactPreview = await invoke("preview_artifact", {
                        id: artifact.id,
                      });
                      artifactLabel = artifact.label;
                    })}>Preview</button
                ><button
                  disabled={!!busy}
                  onclick={() =>
                    run("Showing image", () =>
                      invoke("show_artifact", { id: artifact.id }),
                    )}>Show on stream</button
                >{/if}<button
                disabled={!!busy}
                onclick={() =>
                  run("Opening folder", () =>
                    invoke("reveal_artifact", { id: artifact.id }),
                  )}>Show in folder</button
              >
            </div>
          </div>{/each}
      </section>{/if}
    <footer>
      <span>Bumblebee Desktop <small>PREVIEW</small></span><span
        >Made for the joy of streaming.</span
      >
    </footer>
  </main>
</div>

{#if artifactPreview}<div class="artifact-preview" role="presentation">
    <div>
      <button
        class="quiet"
        aria-label="Close image preview"
        onclick={() => (artifactPreview = null)}>×</button
      >
      <h2>{artifactLabel}</h2>
      <img src={artifactPreview} alt={artifactLabel} />
      <p>This preview is visible only in Bumblebee.</p>
    </div>
  </div>{/if}

{#if drawer}
  <div
    class="drawer-backdrop"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) drawer = false;
    }}
  ></div>
  <aside class="settings-drawer" aria-label="Settings">
    <div class="drawer-title">
      <div>
        <p class="eyebrow">MAKE IT YOURS</p>
        <h2>Settings</h2>
      </div>
      <button
        class="quiet"
        aria-label="Close settings"
        onclick={() => (drawer = false)}>×</button
      >
    </div>
    <nav aria-label="Settings sections">
      {#each [["connections", "Connections"], ["puppets", "Chat puppets"], ["voices", "Voices"], ["overlay", "Overlay"], ["application", "Application"]] as [id, label]}<button
          class:chosen={section === id}
          onclick={() => (section = id)}>{label}</button
        >{/each}
    </nav>
    <div class="settings-content">
      {#if !snapshot}<p>
          Open this interface in the installed desktop app to configure
          Bumblebee.
        </p>{:else if section === "connections"}
        <p class="section-intro">
          Bring your own provider accounts. Each connection is optional. Keys
          and authorization tokens stay in your operating-system keyring.
        </p>
        <section class="settings-card">
          <h3>Azure Speech</h3>
          <p>Natural voices for Bumblebee and your chatters.</p>
          <label
            >Resource region<input
              bind:value={snapshot.settings.azureRegion}
              placeholder="eastus"
            /></label
          ><label
            >Speech key<input
              type="password"
              autocomplete="off"
              bind:value={secretInputs.azure_speech}
              placeholder={snapshot.secrets.azure_speech
                ? "Saved in keyring • enter a replacement"
                : "Paste your Speech resource key"}
            /></label
          >
          <div class="button-row">
            <button
              disabled={!!busy || !secretInputs.azure_speech}
              onclick={() =>
                run("Saving key", () => saveSecret("azure_speech"))}
              >Save key</button
            ><button
              disabled={!!busy}
              onclick={() =>
                run("Validating Speech", () => validate("azure_speech"))}
              >Validate & refresh voices</button
            ><button
              class="subtle"
              onclick={() =>
                openUrl(
                  "https://portal.azure.com/#create/Microsoft.CognitiveServicesSpeechServices",
                )}>Create Speech resource ↗</button
            >
          </div>
        </section>
        <section class="settings-card">
          <h3>OpenAI</h3>
          <p>Bumblebee’s thoughts, tools, and memories.</p>
          <label
            >API key<input
              type="password"
              autocomplete="off"
              bind:value={secretInputs.openai}
              placeholder={snapshot.secrets.openai
                ? "Saved in keyring • enter a replacement"
                : "Paste your API key"}
            /></label
          ><label
            >Model<input
              bind:value={snapshot.settings.openaiModel}
              placeholder="A Responses API model available to your account"
            /></label
          ><label class="check"
            ><input
              type="checkbox"
              bind:checked={snapshot.settings.aiEnabled}
            />Enable Bumblebee’s agent</label
          >
          <div class="button-row">
            <button
              disabled={!!busy || !secretInputs.openai}
              onclick={() => run("Saving key", () => saveSecret("openai"))}
              >Save key</button
            ><button
              disabled={!!busy}
              onclick={() => run("Validating OpenAI", () => validate("openai"))}
              >Validate</button
            >
          </div>
          <h3>Agent permissions</h3>
          <p>
            Allow the tool groups Bumblebee may use on your behalf. Sensitive
            actions still require confirmation. After changing Twitch
            permissions, authorize Twitch again to grant the additional scopes.
          </p>
          {#each [["discord_resources", "Discord channels and messages"], ["discord_moderation", "Discord moderation, roles and voice"], ["twitch_broadcast", "Twitch stream controls, raids, ads and clips"], ["twitch_moderation", "Twitch moderation, chat modes and shoutouts"], ["twitch_polls", "Twitch polls"], ["youtube_moderation", "YouTube moderation"], ["youtube_polls", "YouTube polls"]] as [group, label]}<label
              class="check"
              ><input
                type="checkbox"
                value={group}
                bind:group={snapshot.settings.enabledToolGroups}
              />{label}</label
            >{/each}
        </section>
        <section class="settings-card">
          <h3>Twitch</h3>
          <p>
            Register a <b>public</b> Twitch application, then authorize in your browser.
            Messages are posted as the account you authorize.
          </p>
          <label
            >Client ID<input
              bind:value={snapshot.settings.twitchClientId}
            /></label
          ><label
            >Channel login<input
              bind:value={snapshot.settings.twitchChannel}
              placeholder="your_channel"
            /></label
          >
          <div class="button-row">
            <button
              disabled={!!busy}
              onclick={() =>
                run("Authorizing Twitch", () => authorize("twitch"))}
              >Authorize Twitch</button
            ><button
              class="subtle"
              onclick={() => openUrl("https://dev.twitch.tv/console/apps")}
              >Developer console ↗</button
            >
          </div>
        </section>
        <section class="settings-card">
          <h3>YouTube</h3>
          <p>
            Create a Google OAuth client with the <b>Desktop app</b> type and enable
            YouTube Data API v3. Authorization returns directly to this computer.
          </p>
          <label
            >Desktop client ID<input
              bind:value={snapshot.settings.googleClientId}
            /></label
          ><label
            >Desktop client secret<input
              type="password"
              autocomplete="off"
              bind:value={secretInputs.google_client_secret}
              placeholder={snapshot.secrets.google_client_secret
                ? "Saved in keyring"
                : "From your desktop client configuration"}
            /></label
          ><label
            >Live chat ID<input
              bind:value={snapshot.settings.youtubeLiveChatId}
              placeholder="ID of your broadcast’s active live chat"
            /></label
          >
          <div class="button-row">
            <button
              disabled={!!busy || !secretInputs.google_client_secret}
              onclick={() =>
                run("Saving client", () => saveSecret("google_client_secret"))}
              >Save client secret</button
            ><button
              disabled={!!busy}
              onclick={() =>
                run("Authorizing YouTube", () => authorize("youtube"))}
              >Authorize YouTube</button
            ><button
              class="subtle"
              onclick={() =>
                openUrl("https://console.cloud.google.com/apis/credentials")}
              >Google Cloud console ↗</button
            >
          </div>
        </section>
        <section class="settings-card">
          <h3>Discord</h3>
          <p>
            Discord requires your own bot application. Create one, enable <b
              >Message Content</b
            >
            and <b>Server Members</b> intents, then invite it with View Channels,
            Send Messages, Read Message History, Connect, and Speak permissions.
          </p>
          <label
            >Bot token<input
              type="password"
              autocomplete="off"
              bind:value={secretInputs.discord_bot}
              placeholder={snapshot.secrets.discord_bot
                ? "Saved in keyring"
                : "Paste the token from your app’s Bot page"}
            /></label
          >
          <div class="field-grid">
            <label
              >Server ID<input
                bind:value={snapshot.settings.discordGuildId}
              /></label
            ><label
              >Your Discord user ID<input
                bind:value={snapshot.settings.ownerDiscordId}
              /></label
            ><label
              >Text channel ID<input
                bind:value={snapshot.settings.discordTextChannelId}
              /></label
            ><label
              >Voice channel ID<input
                bind:value={snapshot.settings.discordVoiceChannelId}
              /></label
            >
          </div>
          <small
            >Enable Developer Mode in Discord, then right-click a server,
            channel, or user to copy its ID.</small
          >
          <div class="button-row">
            <button
              disabled={!!busy || !secretInputs.discord_bot}
              onclick={() =>
                run("Saving token", () => saveSecret("discord_bot"))}
              >Save bot token</button
            ><button
              disabled={!!busy}
              onclick={() =>
                run("Validating Discord", () => validate("discord"))}
              >Validate</button
            ><button
              class="subtle"
              onclick={() =>
                openUrl("https://discord.com/developers/applications")}
              >Developer portal ↗</button
            >
          </div>
        </section>
        <section class="settings-card">
          <h3>Saved credentials</h3>
          <p>
            Remove a saved key or authorization to disconnect it. This stops the
            active session.
          </p>
          {#each [["azure_speech", "Azure Speech key"], ["openai", "OpenAI key"], ["discord_bot", "Discord bot token"], ["google_client_secret", "Google client secret"], ["twitch_tokens", "Twitch authorization"], ["google_tokens", "YouTube authorization"]] as [name, label]}
            {#if snapshot.secrets[name]}
              <div class="credential-row">
                <span>{label}</span><button
                  class="compact"
                  disabled={!!busy}
                  onclick={() =>
                    run("Removing credential", async () => {
                      await invoke("delete_secret", { name });
                      await refresh();
                      notice = `${label} removed from this computer.`;
                    })}>Remove</button
                >
              </div>
            {/if}
          {/each}
        </section>
        <section class="settings-card">
          <h3>Discord voice privacy</h3>
          <p>
            Bumblebee listens only to people you permit. Your configured owner
            ID is allowed; blocked IDs always take precedence.
          </p>
          <label class="check"
            ><input
              type="checkbox"
              bind:checked={snapshot.settings.discordListenEveryone}
            />Allow everyone in the connected voice channel</label
          ><label
            >Allowed role IDs (comma-separated)<input
              value={snapshot.settings.discordListenRoleIds.join(", ")}
              onchange={(e) => {
                snapshot!.settings.discordListenRoleIds = e.currentTarget.value
                  .split(/[,\s]+/)
                  .filter(Boolean);
              }}
            /></label
          ><label
            >Allowed user IDs (comma-separated)<input
              value={snapshot.settings.discordListenAllowedUserIds.join(", ")}
              onchange={(e) => {
                snapshot!.settings.discordListenAllowedUserIds =
                  e.currentTarget.value.split(/[,\s]+/).filter(Boolean);
              }}
            /></label
          ><label
            >Blocked user IDs (comma-separated)<input
              value={snapshot.settings.discordListenBlockedUserIds.join(", ")}
              onchange={(e) => {
                snapshot!.settings.discordListenBlockedUserIds =
                  e.currentTarget.value.split(/[,\s]+/).filter(Boolean);
              }}
            /></label
          ><label
            >Wake phrase<select bind:value={snapshot.settings.wakeWord}
              ><option value="hey_bumblebee">Hey Bumblebee</option><option
                value="bumblebee">Bumblebee</option
              ></select
            ></label
          ><label class="check"
            ><input
              type="checkbox"
              bind:checked={snapshot.settings.replayEnabled}
            />Allow a short in-memory voice replay buffer</label
          >{#if snapshot.settings.replayEnabled}<label
              >Replay duration (seconds, maximum 120)<input
                type="number"
                min="5"
                max="120"
                bind:value={snapshot.settings.replaySeconds}
              /></label
            >{/if}<small
            >Voice permission changes are checked before a turn resumes. Saving
            applies permissions immediately and reconnects an active session.</small
          >
        </section>
      {:else if section === "puppets"}
        <label class="check"
          ><input
            type="checkbox"
            bind:checked={snapshot.settings.customImagesEnabled}
          />Allow viewers to submit custom puppet images for approval</label
        >
        <p class="section-intro">
          Viewers use <code>!puppet</code> and <code>!voice</code> in chat. Images
          remain on their current puppet until you approve the exact downloaded image
          below.
        </p>
        <h3>
          Awaiting your approval <span class="count"
            >{snapshot.pendingImages.length}</span
          >
        </h3>
        {#if snapshot.pendingImages.length > 30}<p class="empty-note">
            Showing the first 30 submissions. Reviewing these reveals the next
            submissions.
          </p>{/if}
        {#each snapshot.pendingImages.slice(0, 30) as submission (submission.id)}<div
            class="submission"
          >
            <PendingImage id={submission.id} name={submission.displayName} />
            <div>
              <strong>{submission.displayName}</strong><small
                >{submission.platform} · {new Date(
                  submission.submittedAt,
                ).toLocaleString()}</small
              >
              <div class="button-row">
                <button
                  class="primary compact"
                  disabled={!!busy}
                  onclick={() =>
                    run("Approving image", async () => {
                      await invoke("review_image", {
                        id: submission.id,
                        approve: true,
                      });
                      await refresh();
                    })}>Approve</button
                ><button
                  disabled={!!busy}
                  onclick={() =>
                    run("Rejecting image", async () => {
                      await invoke("review_image", {
                        id: submission.id,
                        approve: false,
                      });
                      await refresh();
                    })}>Reject</button
                >
              </div>
            </div>
          </div>{:else}<p class="empty-note">
            No images waiting for approval.
          </p>{/each}
        <h3>Chatter profiles</h3>
        <form
          class="search-row"
          onsubmit={(e) => {
            e.preventDefault();
            void run("Searching", async () => {
              snapshot!.chatters = await invoke("search_chatters", {
                search: profileSearch,
              });
            });
          }}
        >
          <input
            bind:value={profileSearch}
            placeholder="Search by display name"
            aria-label="Search chatter profiles"
          /><button disabled={!!busy}>Search</button>
        </form>
        {#each snapshot.chatters as chatter}<div class="profile">
            <img src={picture(chatter)} alt="" />
            <div class="profile-details">
              <strong>{chatter.displayName}</strong><small
                >{chatter.platform} · {chatter.voiceId}</small
              >
              <div class="button-row">
                <select
                  aria-label={`Puppet for ${chatter.displayName}`}
                  value={chatter.puppetId}
                  onchange={(e) =>
                    run("Updating puppet", () =>
                      updateChatter(chatter, {
                        puppetId: e.currentTarget.value,
                      }),
                    )}
                  >{#each snapshot.puppets as puppet}<option value={puppet.id}
                      >{puppet.name}</option
                    >{/each}</select
                ><button
                  disabled={!!busy}
                  onclick={() =>
                    run("Resetting puppet", () =>
                      updateChatter(chatter, { puppetId: chatter.puppetId }),
                    )}>Reset image</button
                ><button
                  disabled={!!busy}
                  onclick={() =>
                    run("Changing permissions", () =>
                      updateChatter(chatter, {
                        blocked: !chatter.customizationBlocked,
                      }),
                    )}
                  >{chatter.customizationBlocked
                    ? "Unblock changes"
                    : "Block changes"}</button
                >
              </div>
            </div>
          </div>{:else}<p class="empty-note">
            Profiles appear when viewers first participate.
          </p>{/each}
      {:else if section === "voices"}
        <h3>Bumblebee’s voice</h3>
        <p class="section-intro">
          Bumblebee keeps her own voice. Viewer randomization draws from the
          puppet presets and the regional English catalog.
        </p>
        <label
          >Companion voice<select bind:value={snapshot.settings.bumblebeeVoice}
            >{#each snapshot.voices.filter((v) => v.role === "bumblebee") as voice}<option
                value={voice.id}
                >{voice.name ?? voice.id} · {voice.voiceName}</option
              >{/each}</select
          ></label
        ><label class="check"
          ><input
            type="checkbox"
            bind:checked={snapshot.settings.readChat}
          />Read viewer chat aloud</label
        >
        <h3>Available voices <span class="count">{voices.length}</span></h3>
        <input
          aria-label="Search voices"
          bind:value={voiceSearch}
          placeholder="Search name, voice, or locale"
        />
        <div class="voice-list">
          {#each voices.slice(0, 150) as voice}<div>
              <strong>{voice.name ?? voice.id}</strong><small
                >{voice.voiceName} · rate {voice.rate} · pitch {voice.pitch} · {voice.expression}</small
              >
            </div>{/each}
        </div>
        <p class="empty-note">
          Viewers can use <code>!voice random</code>,
          <code>!voice &lt;name&gt;</code>, or
          <code>!voices &lt;search&gt;</code>.
        </p>
      {:else if section === "overlay"}
        <h3>Bumblebee’s place on your stream</h3>
        <p class="section-intro">
          Positions and sizes are fractions of the overlay viewport. Save to
          update the desktop preview and connected OBS sources immediately.
        </p>
        <label class="check"
          ><input
            type="checkbox"
            bind:checked={snapshot.overlaySettings.beeVisible}
          />Show Bumblebee</label
        >
        <label
          >Horizontal position<input
            type="range"
            min="0"
            max="1"
            step=".01"
            bind:value={snapshot.overlaySettings.beeX}
          /></label
        >
        <label
          >Vertical position<input
            type="range"
            min="0"
            max="1"
            step=".01"
            bind:value={snapshot.overlaySettings.beeY}
          /></label
        >
        <label
          >Size<input
            type="range"
            min=".05"
            max=".8"
            step=".01"
            bind:value={snapshot.overlaySettings.beeScale}
          /></label
        >
        <h3>Chat puppets</h3>
        <label class="check"
          ><input
            type="checkbox"
            bind:checked={snapshot.overlaySettings.puppetsVisible}
          />Show chat puppets</label
        >
        <label
          >Group position<input
            type="range"
            min="0"
            max="1"
            step=".01"
            bind:value={snapshot.overlaySettings.puppetHorizontal}
          /></label
        >
        <label
          >Puppet size<input
            type="range"
            min=".05"
            max=".8"
            step=".01"
            bind:value={snapshot.overlaySettings.puppetScale}
          /></label
        >
        <label
          >Part hidden below the screen<input
            type="range"
            min=".1"
            max="1"
            step=".01"
            bind:value={snapshot.overlaySettings.puppetOcclusion}
          /></label
        >
        <label class="check"
          ><input
            type="checkbox"
            bind:checked={snapshot.overlaySettings.bubblesVisible}
          />Show speech bubbles</label
        >
      {:else}
        <section class="settings-card">
          <h3>OBS overlay</h3>
          <p>
            Only rendering assets, sound, and events are exposed on loopback.
            Changing the token disconnects current overlays; paste the new URL
            into OBS.
          </p>
          <label
            >Local port<input
              type="number"
              min="1024"
              max="65535"
              bind:value={snapshot.settings.overlayPort}
            /></label
          ><small>Port changes apply the next time Bumblebee starts.</small>
          <div class="button-row">
            <button
              disabled={!!busy}
              onclick={() =>
                run("Rotating token", async () => {
                  snapshot!.overlayUrl = await invoke("rotate_overlay_token");
                  notice = "Overlay token replaced. Copy the new URL into OBS.";
                })}>Replace overlay token</button
            ><button
              onclick={() =>
                navigator.clipboard.writeText(snapshot!.overlayUrl)}
              >Copy URL</button
            >
          </div>
        </section>
        <section class="settings-card">
          <h3>On this computer</h3>
          <label class="check"
            ><input
              type="checkbox"
              checked={autostart}
              onchange={(e) =>
                run("Updating autostart", async () => {
                  if (e.currentTarget.checked) await enable();
                  else await disable();
                  autostart = await isEnabled();
                })}
            />Start Bumblebee when I log in</label
          >
          <p>
            Closing the window keeps an active session in the system tray.
            Choose Quit from the tray to disconnect and stop audio.
          </p>
          <button
            disabled={!!busy}
            onclick={() => run("Quitting", () => invoke("quit_app"))}
            >Quit Bumblebee</button
          >
        </section>
      {/if}
    </div>
    <div class="drawer-footer">
      <span>{busy || "Connection changes reconnect an active session."}</span
      ><button
        class="primary"
        disabled={!snapshot || !!busy}
        onclick={() => run("Saving settings", save)}>Save settings</button
      >
    </div>
  </aside>
{/if}
