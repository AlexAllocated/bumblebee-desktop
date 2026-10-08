<script lang="ts">
  import { tick } from "svelte";
  import type { Library, Memory, Reminder } from "../lib/types";
  let {
    library,
    busy,
    invoke,
    run,
    onRefresh,
  }: {
    library: Library | null;
    busy: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
    run: (label: string, action: () => Promise<unknown>) => Promise<void>;
    onRefresh: () => Promise<void>;
  } = $props();
  let memoryDraft = $state("");
  let memoryId = $state<string | null>(null);
  let reminderDraft = $state("");
  let reminderId = $state<string | null>(null);
  let reminderTime = $state("");
  let confirming = $state<string | null>(null);
  let libraryRoot: HTMLDivElement;
  async function requestConfirmation(key: string) {
    confirming = key;
    await tick();
    libraryRoot
      .querySelector<HTMLButtonElement>("[data-confirmation-cancel]")
      ?.focus();
  }
  async function dismissConfirmation() {
    const key = confirming;
    confirming = null;
    await tick();
    if (key)
      libraryRoot
        .querySelector<HTMLButtonElement>(
          `[data-confirmation-trigger="${CSS.escape(key)}"]`,
        )
        ?.focus();
  }
  let memoryInput: HTMLTextAreaElement;
  let reminderInput: HTMLTextAreaElement;
  let focusAfterAction = $state<"memory" | "reminder" | null>(null);
  $effect(() => {
    if (!busy && focusAfterAction) {
      const target = focusAfterAction;
      focusAfterAction = null;
      void tick().then(() =>
        (target === "memory" ? memoryInput : reminderInput)?.focus(),
      );
    }
  });
  const dateLabel = (value: number) => new Date(value).toLocaleString();
  function localDateTime(value: number) {
    const date = new Date(value);
    const local = new Date(date.getTime() - date.getTimezoneOffset() * 60_000);
    return local.toISOString().slice(0, 16);
  }
  function editMemory(memory: Memory) {
    memoryId = memory.id;
    memoryDraft = memory.content;
    memoryInput?.focus();
  }
  function editReminder(reminder: Reminder) {
    reminderId = reminder.id;
    reminderDraft = reminder.content;
    reminderTime = localDateTime(reminder.dueAt);
    reminderInput?.focus();
  }
  async function saveMemory() {
    await run("Saving memory", async () => {
      await invoke("save_memory", {
        id: memoryId,
        content: memoryDraft.trim(),
      });
      memoryId = null;
      memoryDraft = "";
      await onRefresh();
      focusAfterAction = "memory";
    });
  }
  async function saveReminder() {
    await run("Saving reminder", async () => {
      const dueAt = new Date(reminderTime).getTime();
      if (!Number.isFinite(dueAt) || dueAt <= Date.now())
        throw new Error("Choose a future time for the reminder.");
      await invoke("save_reminder", {
        id: reminderId,
        content: reminderDraft.trim(),
        dueAt,
      });
      reminderId = null;
      reminderDraft = "";
      reminderTime = "";
      await onRefresh();
      focusAfterAction = "reminder";
    });
  }
</script>

<div
  bind:this={libraryRoot}
  class="library-settings"
  onchange={(event) => event.stopPropagation()}
>
  <p class="intro">
    Review what Bumblebee remembers and manage scheduled reminders.
  </p>
  <button
    type="button"
    disabled={busy}
    onclick={() => void run("Loading memories and reminders", onRefresh)}
    >Refresh</button
  >
  {#if !library}<p role="status">
      Memories and reminders have not loaded yet.
    </p>{/if}
  <section aria-labelledby="library-memory-title">
    <h3 id="library-memory-title">
      Memories <small>{library?.memories.length ?? 0}</small>
    </h3>
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void saveMemory();
      }}
    >
      <label
        >{memoryId ? "Edit memory" : "New memory"}<textarea
          bind:this={memoryInput}
          bind:value={memoryDraft}
          rows="3"
          maxlength="4000"
          required
          disabled={busy}
          placeholder="What should Bumblebee remember?"
        ></textarea></label
      >
      <div class="actions">
        <button type="submit" disabled={busy || !memoryDraft.trim()}
          >{memoryId ? "Save memory" : "Remember"}</button
        >{#if memoryId}<button
            type="button"
            disabled={busy}
            onclick={() => {
              memoryId = null;
              memoryDraft = "";
            }}>Cancel edit</button
          >{/if}
      </div>
    </form>
    {#each library?.memories ?? [] as memory (memory.id)}
      <article>
        <p class="content">{memory.content}</p>
        <small
          >{memory.actor === "preview:owner" ? "You" : memory.actor} · {dateLabel(
            memory.createdAt,
          )}</small
        >
        {#if confirming === `memory:${memory.id}`}<div class="confirmation">
            <p>Delete this memory?</p>
            <div class="actions">
              <button
                type="button"
                disabled={busy}
                class="danger"
                onclick={() =>
                  void run("Deleting memory", async () => {
                    await invoke("delete_memory", { id: memory.id });
                    confirming = null;
                    if (memoryId === memory.id) {
                      memoryId = null;
                      memoryDraft = "";
                    }
                    await onRefresh();
                    focusAfterAction = "memory";
                  })}>Delete memory</button
              ><button
                type="button"
                disabled={busy}
                data-confirmation-cancel
                onclick={() => void dismissConfirmation()}>Keep it</button
              >
            </div>
          </div>
        {:else}<div class="actions">
            <button
              type="button"
              disabled={busy}
              onclick={() => editMemory(memory)}>Edit</button
            ><button
              type="button"
              disabled={busy}
              data-confirmation-trigger={`memory:${memory.id}`}
              onclick={() => void requestConfirmation(`memory:${memory.id}`)}
              >Delete</button
            >
          </div>{/if}
      </article>
    {:else}{#if library}<p class="empty">No memories yet.</p>{/if}{/each}
  </section>
  <section aria-labelledby="library-reminder-title">
    <h3 id="library-reminder-title">
      Reminders <small>{library?.reminders.length ?? 0}</small>
    </h3>
    <p>
      Times use your computer's timezone. Keep Bumblebee running for delivery.
      New reminders are delivered by Discord DM to your configured Discord
      identity.
    </p>
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void saveReminder();
      }}
    >
      <label
        >{reminderId ? "Edit reminder" : "New reminder"}<textarea
          bind:this={reminderInput}
          bind:value={reminderDraft}
          rows="3"
          maxlength="2000"
          required
          disabled={busy}
          placeholder="What should Bumblebee remind you about?"
        ></textarea></label
      >
      <label
        >When<input
          type="datetime-local"
          bind:value={reminderTime}
          required
          disabled={busy}
        /></label
      >
      <div class="actions">
        <button
          type="submit"
          disabled={busy || !reminderDraft.trim() || !reminderTime}
          >{reminderId ? "Save reminder" : "Schedule reminder"}</button
        >{#if reminderId}<button
            type="button"
            disabled={busy}
            onclick={() => {
              reminderId = null;
              reminderDraft = "";
              reminderTime = "";
            }}>Cancel edit</button
          >{/if}
      </div>
    </form>
    {#each library?.reminders ?? [] as reminder (reminder.id)}
      <article>
        <div class="reminder-heading">
          <strong>{dateLabel(reminder.dueAt)}</strong><small class="state"
            >{reminder.state}</small
          >
        </div>
        <p class="content">{reminder.content}</p>
        <small
          >{reminder.actor === "preview:owner" ? "You" : reminder.actor}</small
        >
        {#if reminder.state === "pending"}
          {#if confirming === `reminder:${reminder.id}`}<div
              class="confirmation"
            >
              <p>Cancel this reminder?</p>
              <div class="actions">
                <button
                  type="button"
                  disabled={busy}
                  class="danger"
                  onclick={() =>
                    void run("Canceling reminder", async () => {
                      await invoke("cancel_reminder", { id: reminder.id });
                      confirming = null;
                      if (reminderId === reminder.id) {
                        reminderId = null;
                        reminderDraft = "";
                        reminderTime = "";
                      }
                      await onRefresh();
                      focusAfterAction = "reminder";
                    })}>Cancel reminder</button
                ><button
                  type="button"
                  disabled={busy}
                  data-confirmation-cancel
                  onclick={() => void dismissConfirmation()}>Keep it</button
                >
              </div>
            </div>
          {:else}<div class="actions">
              <button
                type="button"
                disabled={busy}
                onclick={() => editReminder(reminder)}>Edit</button
              ><button
                type="button"
                disabled={busy}
                data-confirmation-trigger={`reminder:${reminder.id}`}
                onclick={() =>
                  void requestConfirmation(`reminder:${reminder.id}`)}
                >Cancel reminder</button
              >
            </div>{/if}
        {/if}
      </article>
    {:else}{#if library}<p class="empty">No reminders yet.</p>{/if}{/each}
  </section>
</div>

<style>
  .intro,
  p {
    color: var(--muted);
    font-size: 12px;
    line-height: 1.6;
  }
  .intro {
    margin: 0 0 15px;
  }
  section {
    margin-top: 24px;
  }
  h3 {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 0 0 15px;
    color: var(--honey);
    font-size: 14px;
  }
  form,
  article {
    padding: 14px;
    border: 1px solid var(--border);
    border-radius: 12px;
    margin: 12px 0;
    background: #152025;
  }
  label {
    font-size: 12px;
    margin: 0 0 14px;
  }
  textarea {
    resize: vertical;
    width: 100%;
    min-height: 75px;
    border: 1px solid #39484c;
    background: #10191d;
    color: #e9ecec;
    border-radius: 8px;
    padding: 10px;
    font: inherit;
  }
  input {
    width: 100%;
    min-width: 0;
  }
  small {
    color: var(--muted);
    font-size: 10px;
    line-height: 1.55;
    overflow-wrap: anywhere;
  }
  .content {
    margin: 0 0 9px;
    color: #e9ecec;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 12px;
  }
  button {
    font-size: 11px;
    padding: 8px 11px;
  }
  .danger {
    color: #ffb5a8;
    border-color: #925649;
  }
  .confirmation {
    margin-top: 12px;
    border-top: 1px solid #785a43;
  }
  .confirmation p {
    color: #f2d4b4;
  }
  .reminder-heading {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 10px;
    margin-bottom: 12px;
  }
  .reminder-heading strong {
    font-size: 11px;
    color: var(--green);
  }
  .state {
    border: 1px solid var(--border);
    padding: 2px 7px;
    border-radius: 999px;
  }
  .empty {
    text-align: left;
    padding: 4px 0;
  }
</style>
