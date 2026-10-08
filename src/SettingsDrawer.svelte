<script lang="ts">
  import { tick, untrack, type Snippet } from "svelte";

  interface Section {
    id: string;
    label: string;
    description: string;
    icon?: string;
    badge?: string;
  }

  let {
    open,
    section,
    sections,
    onSectionChange,
    onClose,
    saveStatus = "idle",
    saveMessage = "",
    width = 420,
    children,
    footer,
  }: {
    open: boolean;
    section: string;
    sections: Section[];
    onSectionChange: (id: string) => void;
    onClose: () => void;
    saveStatus?: "idle" | "saving" | "saved" | "error";
    saveMessage?: string;
    width?: 420 | 560;
    children: Snippet;
    footer?: Snippet;
  } = $props();

  const id = $props.id();
  let dialog = $state<HTMLDialogElement | null>(null);
  let body = $state<HTMLDivElement | null>(null);
  let lastSection = untrack(() => section);
  let backdropPointer = false;
  const title = $derived(
    section === "home"
      ? "Settings"
      : (sections.find((item) => item.id === section)?.label ?? "Settings"),
  );
  const statusText = $derived(
    saveMessage ||
      { idle: "", saving: "Saving…", saved: "Saved", error: "Save failed" }[
        saveStatus
      ],
  );

  function focusable(element: HTMLElement) {
    return Array.from(
      element.querySelectorAll<HTMLElement>(
        "button, [href], input, select, textarea, [tabindex]",
      ),
    ).filter(
      (target) =>
        target.tabIndex >= 0 &&
        !target.matches(":disabled, [hidden], [inert]") &&
        !target.closest("[hidden], [inert]") &&
        target.getClientRects().length > 0,
    );
  }

  function trapFocus(event: KeyboardEvent) {
    if (event.key !== "Tab" || !dialog) return;
    const controls = focusable(dialog);
    const first = controls[0];
    const last = controls.at(-1);
    if (!first || !last) {
      event.preventDefault();
      dialog.focus();
      return;
    }
    const active = document.activeElement;
    if (!controls.includes(active as HTMLElement)) {
      event.preventDefault();
      (event.shiftKey ? last : first).focus();
    } else if (event.shiftKey && active === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function outsideDialog(event: MouseEvent | PointerEvent) {
    if (!dialog || event.target !== dialog) return false;
    const rect = dialog.getBoundingClientRect();
    return (
      event.clientX < rect.left ||
      event.clientX > rect.right ||
      event.clientY < rect.top ||
      event.clientY > rect.bottom
    );
  }

  $effect(() => {
    if (!open || !dialog) return;
    const element = dialog;
    const trigger = document.activeElement as HTMLElement | null;
    const overflow = document.body.style.overflow;
    let disposed = false;
    document.body.style.overflow = "hidden";
    element.showModal();
    void tick().then(() => {
      if (!disposed && element.open)
        element
          .querySelector<HTMLElement>("[data-drawer-initial-focus]")
          ?.focus();
    });
    return () => {
      disposed = true;
      element.close();
      document.body.style.overflow = overflow;
      if (trigger?.isConnected) trigger.focus({ preventScroll: true });
    };
  });

  $effect(() => {
    const current = section;
    const previous = lastSection;
    lastSection = current;
    if (!open || !dialog || previous === current) return;
    const element = dialog;
    void tick().then(() => {
      if (!element.open || section !== current) return;
      if (body) body.scrollTop = 0;
      const card =
        current === "home"
          ? Array.from(
              element.querySelectorAll<HTMLElement>("[data-settings-section]"),
            ).find((item) => item.dataset.settingsSection === previous)
          : null;
      (card ?? element.querySelector<HTMLElement>("h2"))?.focus({
        preventScroll: true,
      });
    });
  });
</script>

{#if open}
  <dialog
    bind:this={dialog}
    class="settings-dialog"
    style={`--settings-width: ${width}px`}
    aria-labelledby={`${id}-title`}
    oncancel={(event) => {
      event.preventDefault();
      onClose();
    }}
    onkeydown={trapFocus}
    onpointerdown={(event) => (backdropPointer = outsideDialog(event))}
    onclick={(event) => {
      if (backdropPointer && outsideDialog(event)) onClose();
      backdropPointer = false;
    }}
  >
    <div class="settings-heading">
      <div class="settings-heading-main">
        {#if section !== "home"}
          <button
            class="settings-back"
            onclick={() => onSectionChange("home")}
            aria-label="Back to settings"
            ><span aria-hidden="true">←</span> Back</button
          >
        {/if}
        <h2 id={`${id}-title`} tabindex="-1">{title}</h2>
      </div>
      <button
        class="settings-close"
        onclick={onClose}
        aria-label="Close settings"
        data-drawer-initial-focus><span aria-hidden="true">×</span></button
      >
    </div>
    <div
      class="settings-save-status"
      class:failed={saveStatus === "error"}
      class:saving={saveStatus === "saving"}
      role="status"
      aria-live="polite"
      aria-atomic="true"
    >
      {statusText}
    </div>
    <div class="settings-body" bind:this={body}>
      {#if section === "home"}
        <nav class="settings-menu" aria-label="Settings sections">
          {#each sections as item (item.id)}
            <button
              class="settings-menu-card"
              data-settings-section={item.id}
              onclick={() => onSectionChange(item.id)}
            >
              {#if item.icon}<span class="settings-menu-icon" aria-hidden="true"
                  >{item.icon}</span
                >{/if}
              <span class="settings-menu-text">
                <strong>{item.label}</strong>
                <span>{item.description}</span>
                {#if item.badge}<small>{item.badge}</small>{/if}
              </span>
              <span class="settings-chevron" aria-hidden="true">›</span>
            </button>
          {/each}
        </nav>
      {:else}
        {@render children()}
      {/if}
    </div>
    {#if footer}<div class="settings-footer">{@render footer()}</div>{/if}
  </dialog>
{/if}

<style>
  .settings-dialog {
    position: fixed;
    inset: 0 0 0 auto;
    width: min(var(--settings-width), 100vw);
    max-width: 100vw;
    height: 100dvh;
    max-height: 100dvh;
    margin: 0;
    padding: 0;
    border: 0;
    border-left: 1px solid var(--border, #293439);
    border-radius: 0;
    background: var(--panel, #182024);
    color: #e9ecec;
    box-shadow: -12px 0 55px #0005;
    overflow: hidden;
  }
  .settings-dialog[open] {
    display: flex;
    flex-direction: column;
  }
  .settings-dialog::backdrop {
    background: #080e12a6;
    backdrop-filter: blur(7px);
  }
  .settings-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    padding: 18px 20px 12px;
    border-bottom: 1px solid var(--border, #293439);
  }
  .settings-heading-main {
    min-width: 0;
  }
  h2 {
    margin: 0;
    font-size: 17px;
    line-height: 1.35;
    font-weight: 650;
    letter-spacing: 0;
    overflow-wrap: anywhere;
  }
  h2:focus {
    outline: none;
  }
  .settings-close {
    flex-shrink: 0;
    width: 36px;
    height: 36px;
    padding: 0;
    font-size: 25px;
    line-height: 1;
    border-radius: 50%;
    background: transparent;
  }
  .settings-back {
    padding: 0;
    border: 0;
    margin: 0 0 9px;
    color: var(--muted, #95a2a5);
    background: transparent;
    font-size: 12px;
  }
  .settings-back:hover {
    color: var(--honey, #f5ca69);
  }
  .settings-save-status {
    flex-shrink: 0;
    color: var(--green, #8cd2b2);
    font-size: 11px;
    line-height: 1.5;
    padding: 10px 20px 0;
    overflow-wrap: anywhere;
  }
  .settings-save-status:empty {
    padding: 0;
  }
  .settings-save-status.saving {
    color: var(--honey, #f5ca69);
  }
  .settings-save-status.failed {
    color: #ffb5a8;
  }
  .settings-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    padding: 18px 20px 24px;
    scrollbar-gutter: stable;
  }
  .settings-menu {
    display: grid;
    gap: 10px;
    padding: 0;
    border: 0;
  }
  .settings-menu-card {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 15px 13px;
    border: 1px solid #35444a;
    border-radius: 14px;
    background: #213035;
    color: #e9ecec;
    text-align: left;
  }
  .settings-menu-card:hover {
    border-color: #7d7455;
    background: #2a383c;
  }
  .settings-menu-icon {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 35px;
    height: 35px;
    border-radius: 11px;
    background: #e3c67914;
    color: var(--honey, #f5ca69);
    font-size: 20px;
  }
  .settings-menu-text {
    display: flex;
    flex: 1;
    min-width: 0;
    flex-direction: column;
    gap: 5px;
    overflow-wrap: anywhere;
  }
  .settings-menu-text strong {
    font-size: 13px;
    font-weight: 650;
  }
  .settings-menu-text > span {
    font-size: 11px;
    line-height: 1.5;
    color: #acbabc;
  }
  .settings-menu-text small {
    width: fit-content;
    color: var(--green, #8cd2b2);
    font-size: 10px;
    line-height: 1.5;
  }
  .settings-chevron {
    flex-shrink: 0;
    color: var(--muted, #95a2a5);
    font-size: 23px;
  }
  .settings-footer {
    flex-shrink: 0;
    padding: 15px 20px;
    border-top: 1px solid var(--border, #293439);
    background: #1c272b;
  }
  @media (prefers-reduced-motion: no-preference) {
    .settings-dialog[open] {
      animation: settings-enter 160ms ease-out;
    }
    @keyframes settings-enter {
      from {
        transform: translateX(24px);
        opacity: 0.5;
      }
      to {
        transform: translateX(0);
        opacity: 1;
      }
    }
  }
</style>
