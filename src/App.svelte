<script lang="ts">
  import { onMount } from "svelte";
  import { isTauri } from "@tauri-apps/api/core";
  import Stage from "./Stage.svelte";
  import SettingsDrawer from "./SettingsDrawer.svelte";
  import OverlayControls from "./OverlayControls.svelte";
  import PlatformSettings from "./settings/PlatformSettings.svelte";
  import ChatterOverrides from "./settings/ChatterOverrides.svelte";
  import ResetPreferences, {
    type ResetScope,
  } from "./settings/ResetPreferences.svelte";
  import AudioSettings from "./settings/AudioSettings.svelte";
  import AiSettings from "./settings/AiSettings.svelte";
  import LibrarySettings from "./settings/LibrarySettings.svelte";
  import { desktopBackend, type DesktopBackend } from "./lib/desktop";
  import { createSettingsWriter } from "./lib/settingsWriter";
  import {
    subjectLabels,
    type OverlaySubject,
    type OverlaySettingsPatch,
    type OverlaySettings,
  } from "./lib/overlay";
  import type { Settings, Library } from "./lib/types";
  let { backend = desktopBackend }: { backend?: DesktopBackend } = $props();
  const invoke = <T,>(command: string, args?: Record<string, unknown>) =>
    backend.invoke<T>(command, args);
  const listen: DesktopBackend["listen"] = (...args) => backend.listen(...args);
  const openUrl: DesktopBackend["openUrl"] = (...args) =>
    backend.openUrl(...args);
  const enable = () => backend.autostart.enable();
  const disable = () => backend.autostart.disable();
  const isEnabled = () => backend.autostart.isEnabled();
  import PendingImage from "./PendingImage.svelte";
  import type {
    Chatter,
    OverlayEvent,
    Snapshot,
    ProviderStatus,
  } from "./lib/types";
  let snapshot: Snapshot | null = $state(null);
  let drawer = $state(false);
  let section = $state("home");
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
  let saveStatus = $state<"idle" | "saving" | "saved" | "error">("idle");
  let saveError = $state("");
  let library = $state<Library | null>(null);
  let models = $state<string[]>([]);
  let overlaySubject = $state<OverlaySubject>("bumblebee");
  let rotationPending = $state(false);
  let showSpeechPreview = $state(false);

  const sections = [
    {
      id: "discord",
      label: "Discord",
      description: "Server, channels, chat permissions and voice listening.",
      icon: "◖",
    },
    {
      id: "twitch",
      label: "Twitch",
      description: "Your channel, chat permissions and relay.",
      icon: "◩",
    },
    {
      id: "youtube",
      label: "YouTube",
      description: "Your broadcast, chat permissions and relay.",
      icon: "▶",
    },
    {
      id: "audio",
      label: "Chat and audio",
      description: "Voices, output, volume, queue and interruption.",
      icon: "♫",
    },
    {
      id: "overlay",
      label: "Overlay",
      description: "Puppets, speech bubbles, anchors and visibility.",
      icon: "▣",
    },
    {
      id: "ai",
      label: "AI",
      description: "Models, reasoning and Bumblebee’s tool permissions.",
      icon: "✦",
    },
    {
      id: "library",
      label: "Memories and reminders",
      description: "Review what Bumblebee remembers and schedules.",
      icon: "◷",
    },
    {
      id: "puppets",
      label: "Overrides and chatters",
      description: "Per-viewer permissions, puppets and image approval.",
      icon: "♙",
    },
    {
      id: "voices",
      label: "Voice catalog",
      description: "Curated personalities and regional English voices.",
      icon: "≋",
    },
    {
      id: "application",
      label: "Application",
      description: "OBS connection, saved credentials and startup.",
      icon: "⚙",
    },
  ];
  const settingsWriter = createSettingsWriter<Settings>({
    read: () => snapshot!.settings,
    apply: (settings) => {
      if (snapshot) snapshot.settings = settings;
    },
    persist: (patch) => invoke<Settings>("patch_settings", { patch }),
    status: (state, message) => {
      saveStatus = state;
      saveError = message ?? "";
    },
  });
  async function refresh() {
    const next = await invoke<Snapshot>("get_snapshot");
    if (snapshot) {
      settingsWriter.accept(next.settings);
      next.settings = snapshot.settings;
    } else {
      snapshot = next;
      settingsWriter.accept(next.settings);
    }
    if (profileSearch.trim())
      next.chatters = await invoke<Chatter[]>("search_chatters", {
        search: profileSearch,
      });
    snapshot = next;
    statuses = next.statuses;
    if (next.credentialStoreError || next.overlayError)
      error = next.credentialStoreError ?? next.overlayError ?? "";
  }
  function settingsChanged() {
    settingsWriter.schedule();
  }
  async function closeSettings() {
    try {
      await settingsWriter.flush();
      drawer = false;
    } catch {}
  }
  function openSection(id: string) {
    section = id;
    drawer = true;
    if (id === "library") void run("Loading memories", loadLibrary);
  }
  async function loadLibrary() {
    library = await invoke<Library>("get_library");
  }
  async function loadModels() {
    models = await invoke<string[]>("openai_models");
  }
  let overlaySave: Promise<unknown> = Promise.resolve();
  function commitOverlay(
    patch: OverlaySettingsPatch,
  ): Promise<OverlaySettings> {
    saveStatus = "saving";
    saveError = "";
    const operation = overlaySave
      .catch(() => {})
      .then(async () => {
        const settings = await invoke<OverlaySettings>(
          "patch_overlay_settings",
          { patch },
        );
        if (snapshot) snapshot.overlaySettings = settings;
        saveStatus = "saved";
        return settings;
      })
      .catch((e) => {
        saveStatus = "error";
        saveError = String(e);
        throw e;
      });
    overlaySave = operation;
    return operation;
  }
  async function resetPreferences(scope: ResetScope) {
    if (busy) throw new Error("Wait for the current operation to finish.");
    busy = "Resetting preferences";
    try {
      await settingsWriter.flush();
      await overlaySave.catch(() => {});
      const result = await invoke<{
        settings: Settings;
        overlaySettings: OverlaySettings | null;
      }>("reset_preferences", { scope });
      settingsWriter.accept(result.settings);
      if (snapshot && result.overlaySettings)
        snapshot.overlaySettings = result.overlaySettings;
    } finally {
      busy = "";
    }
  }
  async function storeSecret(name: string, value: string) {
    secretInputs[name] = value;
    await saveSecret(name);
  }
  function providerLabel(id: string) {
    return (
      (
        {
          azure_speech: "Speech",
          openai: "OpenAI",
          twitch: "Twitch",
          youtube: "YouTube",
          discord: "Discord",
        } as Record<string, string>
      )[id] ?? id
    );
  }
  function providerState(id: string) {
    const status = statuses.find((s) => s.provider === id);
    if (status) return status.state;
    const secret =
      (
        {
          twitch: "twitch_tokens",
          youtube: "google_tokens",
          discord: "discord_bot",
        } as Record<string, string>
      )[id] ?? id;
    return snapshot?.secrets[secret] ? "Configured" : "Not configured";
  }
  async function copyOverlay() {
    if (snapshot) {
      await navigator.clipboard.writeText(snapshot.overlayUrl);
      notice = "Overlay URL copied. Paste it into your OBS Browser Source.";
    }
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
    await settingsWriter.flush();
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
    if (!backend.available) return;
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
      settingsWriter.dispose();
      clearInterval(activityTimer);
      unlisten.forEach((fn) => fn());
    };
  });
</script>

<div class="app-shell dashboard-workspace">
  <header class="desktop-header">
    <div class="brand">
      <img src="./bumblebee.png" alt="" />
      <div><strong>Bumblebee</strong><span>STREAMER DASHBOARD</span></div>
    </div>
    <div class="header-actions">
      <span class:live={snapshot?.active} class="session-indicator"
        ><i></i>{snapshot?.active
          ? "Session running"
          : "Ready when you are"}</span
      >
      <button
        class="primary compact"
        disabled={!snapshot || !!busy}
        onclick={() =>
          run("Session", async () => {
            await save();
            await invoke(snapshot?.active ? "stop_session" : "start_session");
            await refresh();
          })}>{snapshot?.active ? "End session" : "Start session"}</button
      >
    </div>
  </header>
  <main class="workspace-main">
    {#if error}<div role="alert" class="banner error">
        <span>{error}</span><button
          onclick={() => (error = "")}
          aria-label="Dismiss error">×</button
        >
      </div>{/if}
    {#if notice}<div role="status" class="banner notice">
        <span>{notice}</span><button
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
          <p class="eyebrow">INTERRUPTED REQUEST</p>
          {#if turn.resumable}
            <p>
              A request from {turn.actor} stopped before it finished. Resume continues
              the saved work. Actions with an uncertain outcome stay blocked from
              being repeated.
            </p>
            {#if !snapshot?.active}<p>
                Start a session to resume this request.
              </p>{/if}
          {:else}
            <p>
              A request from {turn.actor} stopped before its outcome was known. Check
              the affected service before making a new request. Bumblebee cannot safely
              resume this request.
            </p>
          {/if}
          {#if turn.receipts.length}
            <details>
              <summary>Delivery receipts</summary>
              {#each turn.receipts as receipt}
                <p>
                  {receipt.destination ?? "Saved destination"}: {receipt.status}.
                  Confirmed {receipt.completedParts ??
                    receipt.messageIds.length}{#if receipt.totalParts !== null}
                    of {receipt.totalParts}{/if} message parts.
                  {#if receipt.unacknowledgedPartMayHaveSent}
                    An additional part may have been sent.
                  {/if}
                </p>
                {#if receipt.messageIds.length}
                  <p>
                    Message IDs: <code>{receipt.messageIds.join(", ")}</code>
                  </p>
                {/if}
              {/each}
            </details>
          {/if}
        </div>
        {#if turn.resumable}<button
            disabled={!!busy || !snapshot?.active}
            onclick={() =>
              run("Resuming", async () => {
                await invoke("resume_interrupted", { id: turn.id });
                await activity();
              })}>Resume</button
          >{/if}
        <button
          disabled={!!busy}
          onclick={() =>
            run("Acknowledging", async () => {
              await invoke("dismiss_interrupted", { id: turn.id });
              await activity();
            })}>{turn.resumable ? "Cancel request" : "Acknowledge"}</button
        >
      </section>{/each}

    <section class="overlay-workspace panel" aria-label="Overlay editor">
      <div class="overlay-toolbar">
        <div class="overlay-address">
          <span aria-hidden="true">↗</span><input
            aria-label="OBS overlay URL"
            value={snapshot?.overlayUrl ?? ""}
            readonly
            placeholder="Local overlay URL"
            onclick={(e) => e.currentTarget.select()}
          /><button
            class="compact"
            disabled={!snapshot || !!snapshot.overlayError}
            onclick={() => run("Copying overlay URL", copyOverlay)}>Copy</button
          ><button
            class="quiet compact"
            title="Regenerate overlay URL"
            aria-label="Regenerate overlay URL"
            disabled={!snapshot}
            onclick={() => (rotationPending = !rotationPending)}>↻</button
          >
        </div>
        <button
          class="settings-button"
          onclick={() => openSection("home")}
          aria-label="Open settings">⚙ Settings</button
        >
      </div>
      {#if rotationPending}<div class="rotation-confirm" role="alert">
          <span
            >Replace the overlay URL? Existing OBS sources will need the new
            URL.</span
          ><button
            onclick={() =>
              run("Replacing overlay URL", async () => {
                snapshot!.overlayUrl = await invoke("rotate_overlay_token");
                rotationPending = false;
                notice = "Overlay URL replaced. Copy it into OBS.";
              })}>Replace URL</button
          ><button class="quiet" onclick={() => (rotationPending = false)}
            >Cancel</button
          >
        </div>{/if}
      <div class="editor-canvas">
        {#if snapshot}{#key assetBase}<Stage
              {assetBase}
              {subscribe}
              muted={previewMuted}
              editing={true}
              settings={snapshot.overlaySettings}
              onCommit={commitOverlay}
              onReady={rendererReady}
              onError={rendererResult}
            />{/key}{:else}<div class="unavailable">
            <img src="./bumblebee.png" alt="Bumblebee" />
            <p>Loading your workspace…</p>
          </div>{/if}
      </div>
      <div class="editor-footer">
        <span
          >Drag to position · resize with handles · changes save automatically</span
        >
        <div>
          <button class="subtle" onclick={() => (previewMuted = !previewMuted)}
            >{previewMuted ? "Unmute preview" : "Mute preview"}</button
          ><button
            class="subtle"
            onclick={() => (showSpeechPreview = !showSpeechPreview)}
            >Test speech</button
          ><button
            class="quiet compact"
            disabled={!snapshot}
            onclick={() =>
              run("Stopping speech", () => invoke("cancel_speech"))}
            >■ Stop speech</button
          >
        </div>
      </div>
      {#if showSpeechPreview}<form
          class="preview-input"
          onsubmit={(e) => {
            e.preventDefault();
            void run("Previewing speech", async () => {
              await save();
              await invoke("preview_speech", { text: previewText });
            });
          }}
        >
          <input
            aria-label="Speech preview text"
            bind:value={previewText}
            maxlength={500}
          /><button class="primary compact" disabled={!snapshot || !!busy}
            >Say it</button
          >
        </form>{/if}
    </section>
    <div class="connection-strip" aria-label="Connections">
      {#each ["twitch", "youtube", "discord", "azure_speech", "openai"] as provider}<button
          title={statuses.find((s) => s.provider === provider)?.message}
          onclick={() =>
            openSection(
              provider === "azure_speech"
                ? "audio"
                : provider === "openai"
                  ? "ai"
                  : provider,
            )}
          ><i class:connected={providerState(provider) === "connected"}
          ></i><strong>{providerLabel(provider)}</strong><span
            >{providerState(provider).replaceAll("_", " ")}</span
          ></button
        >{/each}
    </div>
    <details class="panel conversation-panel">
      <summary
        >Conversation <span
          >{log.length ? `${log.length} events` : "This session"}</span
        ></summary
      >
      <div class="activity-log" aria-live="polite">
        {#each log as line}<div class="log-line">
            <time>{line.time}</time><strong>{line.sender}</strong><span
              >{line.text}</span
            >
          </div>{:else}<p class="empty-note">
            Conversation appears here when a session is running.
          </p>{/each}
      </div>
    </details>
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

<SettingsDrawer
  open={drawer}
  {section}
  {sections}
  onSectionChange={openSection}
  onClose={() => void closeSettings()}
  {saveStatus}
  saveMessage={saveError}
  width={560}
>
  <fieldset
    class="settings-body settings-fields"
    disabled={!!busy}
    onchange={settingsChanged}
    aria-label="Settings controls"
  >
    {#if error}<div class="banner error" role="alert">
        {error}<button aria-label="Dismiss error" onclick={() => (error = "")}
          >×</button
        >
      </div>{/if}
    {#if notice}<div class="banner notice" role="status">
        {notice}<button
          aria-label="Dismiss notice"
          onclick={() => (notice = "")}>×</button
        >
      </div>{/if}
    {#if !snapshot}<p>Loading settings…</p>
    {:else if section === "discord" || section === "twitch" || section === "youtube"}
      {#key section}<PlatformSettings
          platform={section}
          settings={snapshot.settings}
          saved={snapshot.secrets}
          busy={!!busy}
          {invoke}
          {run}
          onSaveSecret={storeSecret}
          onAuthorize={authorize}
          onValidate={validate}
        />{/key}
      {#if section === "discord"}
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
          >
        </section>
      {/if}
    {:else if section === "audio"}
      <AudioSettings
        settings={snapshot.settings}
        voices={snapshot.voices}
        busy={!!busy}
        onPreview={() =>
          run("Previewing speech", async () => {
            await save();
            await invoke("preview_speech", { text: previewText });
          })}
      />
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
            onclick={() => run("Saving key", () => saveSecret("azure_speech"))}
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
    {:else if section === "ai"}
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
      </section>

      <AiSettings
        settings={snapshot.settings}
        busy={!!busy}
        {models}
        onLoadModels={() => run("Loading models", loadModels)}
      />
    {:else if section === "library"}
      <LibrarySettings
        {library}
        busy={!!busy}
        {invoke}
        {run}
        onRefresh={loadLibrary}
      />
    {:else if section === "overlay"}
      <p>
        These controls edit the same layout shown on the dashboard and in OBS.
      </p>
      <label
        >Element<select bind:value={overlaySubject}
          >{#each Object.entries(subjectLabels) as [id, label]}<option
              value={id}>{label}</option
            >{/each}</select
        ></label
      >
      <OverlayControls
        settings={snapshot.overlaySettings}
        subject={overlaySubject}
        onCommit={commitOverlay}
      />
    {:else if section === "puppets"}
      <label class="check"
        ><input
          type="checkbox"
          bind:checked={snapshot.settings.customImagesEnabled}
        />Allow viewers to submit custom puppet images for approval</label
      >
      <p class="section-intro">
        Viewers use <code>!puppet</code> and <code>!voice</code> in chat. Images remain
        on their current puppet until you approve the exact downloaded image below.
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
          <img
            src={picture(chatter)}
            alt=""
            onerror={(event) => {
              const fallback =
                assetBase + `puppets/images/${chatter.puppetId}.png`;
              const img = event.currentTarget as HTMLImageElement;
              if (img.src !== fallback) img.src = fallback;
            }}
          />
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
            <ChatterOverrides
              {chatter}
              busy={!!busy}
              onSave={async (overrides) => {
                await updateChatter(chatter, { overrides });
              }}
            />
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
        ><input type="checkbox" bind:checked={snapshot.settings.readChat} />Read
        viewer chat aloud</label
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
    {:else if section === "application"}
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
            onclick={() => navigator.clipboard.writeText(snapshot!.overlayUrl)}
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
          Closing the window keeps an active session in the system tray. Choose
          Quit from the tray to disconnect and stop audio.
        </p>
        <button
          disabled={!!busy}
          onclick={() => run("Quitting", () => invoke("quit_app"))}
          >Quit Bumblebee</button
        >
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
    {/if}
    {#if snapshot && ["audio", "ai", "discord", "twitch", "youtube", "application"].includes(section)}
      {#key section}<ResetPreferences
          scope={section === "application" ? "all" : (section as ResetScope)}
          busy={!!busy}
          onReset={resetPreferences}
        />{/key}
    {/if}
  </fieldset>
  {#snippet footer()}
    <div class="settings-footer-actions">
      <span>{busy || "Settings save automatically."}</span><button
        disabled={!snapshot || !!busy}
        onclick={() => run("Saving settings", save)}
        >{saveStatus === "error" ? "Retry save" : "Save now"}</button
      >
    </div>
  {/snippet}
</SettingsDrawer>
