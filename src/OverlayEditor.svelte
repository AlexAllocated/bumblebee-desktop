<script lang="ts">
  import OverlayControls from "./OverlayControls.svelte";
  import {
    applyOverlayPatch,
    cloneOverlaySettings,
    defaultOverlaySettings,
    subjectLabels,
    type OverlaySettings,
    type OverlaySettingsPatch,
    type OverlaySubject,
  } from "./lib/overlay";
  import {
    subjectRect,
    subjectEnabled,
    rect,
    placementPatch,
    resizePatch,
    streamerPose,
    tailPatch,
    streamerSubjectPatch,
    subjectCircleForPose,
    type Viewport,
    type Dimensions,
    type ResizeDirection,
  } from "./renderer/overlayLayout";
  import { resolveEditPlaceholderTabSide } from "./renderer/editPlaceholderTabs";
  let {
    settings,
    viewport,
    dimensions,
    onPreview,
    onCommit,
    onCancel,
  }: {
    settings: OverlaySettings;
    viewport: Viewport;
    dimensions: Dimensions;
    onPreview: (settings: OverlaySettings) => void;
    onCancel?: () => void;
    onCommit: (patch: OverlaySettingsPatch) => Promise<OverlaySettings | void>;
  } = $props();
  const subjects: OverlaySubject[] = [
    "bumblebee",
    "puppet",
    "streamerVoiceBubble",
  ];
  const colors: Record<OverlaySubject, string> = {
    bumblebee: "#fde047",
    puppet: "#f2c94c",
    streamerVoiceBubble: "#7dd3fc",
  };
  let root: HTMLDivElement;
  let selected = $state<OverlaySubject | null>(null);
  let options = $state<OverlaySubject | null>(null);
  let error = $state("");
  let drag = $state<{
    id: number;
    subject: OverlaySubject;
    mode: "move" | "tail" | "subject" | "radius" | ResizeDirection;
    x: number;
    y: number;
    start: OverlaySettings;
    rect: ReturnType<typeof subjectRect>;
  } | null>(null);
  const local = (e: PointerEvent) => {
    const r = root.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };
  function start(
    e: PointerEvent,
    subject: OverlaySubject,
    mode: "move" | "tail" | "subject" | "radius" | ResizeDirection,
  ) {
    if (e.button !== 0 && e.pointerType !== "touch") return;
    e.preventDefault();
    e.stopPropagation();
    options = null;
    selected = subject;
    const p = local(e);
    drag = {
      id: e.pointerId,
      subject,
      mode,
      x: p.x,
      y: p.y,
      start: cloneOverlaySettings(settings),
      rect: subjectRect(settings, subject, viewport, dimensions),
    };
    root.setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    if (!drag || drag.id !== e.pointerId) return;
    e.preventDefault();
    const p = local(e);
    let patch: OverlaySettingsPatch;
    if (drag.mode === "tail") patch = tailPatch(drag.start, p, viewport);
    else if (drag.mode === "subject" || drag.mode === "radius") {
      const circle = subjectCircleForPose(streamerPose(drag.start, viewport));
      const center =
        drag.mode === "subject"
          ? {
              x: circle.center.x + p.x - drag.x,
              y: circle.center.y + p.y - drag.y,
            }
          : circle.center;
      patch = streamerSubjectPatch(
        drag.start,
        center,
        drag.mode === "radius"
          ? Math.hypot(p.x - center.x, p.y - center.y)
          : circle.radius,
        viewport,
      );
    } else if (drag.mode === "move")
      patch = placementPatch(
        drag.start,
        drag.subject,
        rect(
          drag.rect.left + p.x - drag.x,
          drag.rect.top + p.y - drag.y,
          drag.rect.width,
          drag.rect.height,
        ),
        viewport,
      );
    else
      patch = resizePatch(
        drag.start,
        drag.subject,
        drag.rect,
        drag.mode,
        p,
        viewport,
        dimensions,
      );
    onPreview(applyOverlayPatch(drag.start, patch));
  }
  async function commit(patch: OverlaySettingsPatch) {
    error = "";
    try {
      return await onCommit(patch);
    } catch (e) {
      error = String(e);
      throw e;
    }
  }
  function finish(e: PointerEvent, cancel = false) {
    if (!drag || drag.id !== e.pointerId) return;
    const session = drag;
    drag = null;
    if (root.hasPointerCapture(e.pointerId))
      root.releasePointerCapture(e.pointerId);
    if (cancel) {
      onPreview(session.start);
      onCancel?.();
      return;
    }
    void commit({ [session.subject]: settings[session.subject] }).catch(() =>
      onPreview(session.start),
    );
  }
  function key(e: KeyboardEvent, subject: OverlaySubject) {
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(e.key))
      return;
    e.preventDefault();
    const r = subjectRect(settings, subject, viewport, dimensions);
    const step = e.shiftKey ? 10 : 1;
    const patch = placementPatch(
      settings,
      subject,
      rect(
        r.left +
          (e.key === "ArrowRight" ? step : e.key === "ArrowLeft" ? -step : 0),
        r.top +
          (e.key === "ArrowDown" ? step : e.key === "ArrowUp" ? -step : 0),
        r.width,
        r.height,
      ),
      viewport,
    );
    onPreview(applyOverlayPatch(settings, patch));
    void commit(patch).catch(() => {});
  }
  function enabled(subject: OverlaySubject, value: boolean) {
    void commit({
      [subject]: { [subject === "bumblebee" ? "visible" : "enabled"]: value },
    }).catch(() => {});
  }
  function reset(subject: OverlaySubject, kind: "position" | "size") {
    const defaults = defaultOverlaySettings[subject];
    const patch =
      kind === "position"
        ? {
            position: defaults.position,
            anchor: defaults.anchor,
            ...(subject === "puppet" ? { occlusionPercentage: 0.3 } : {}),
          }
        : subject === "streamerVoiceBubble"
          ? { chatBubbleMaxWidthPercent: 35, chatBubbleMaxHeightPercent: 25 }
          : {
              scalePercentage: defaultOverlaySettings[subject].scalePercentage,
            };
    void commit({ [subject]: patch }).catch(() => {});
  }
</script>

<div
  class="overlay-editor"
  bind:this={root}
  onpointermove={move}
  onpointerup={(e) => finish(e)}
  onpointercancel={(e) => finish(e, true)}
  onlostpointercapture={(e) => {
    if (drag && drag.id === e.pointerId) finish(e, true);
  }}
  role="application"
  aria-label="Overlay edit surface"
>
  <div class="element-list" aria-label="Overlay elements">
    {#each subjects as subject}<button
        class:enabled={subjectEnabled(settings, subject)}
        aria-pressed={subjectEnabled(settings, subject)}
        onclick={() => enabled(subject, !subjectEnabled(settings, subject))}
        title={`${subjectEnabled(settings, subject) ? "Hide" : "Show"} ${subjectLabels[subject]}`}
        >{subjectLabels[subject]}</button
      >{/each}
  </div>
  {#if viewport.width && viewport.height}
    {#each subjects as subject}
      {#if subjectEnabled(settings, subject)}
        {@const box = subjectRect(settings, subject, viewport, dimensions)}
        {@const side = resolveEditPlaceholderTabSide(box, 30, viewport)}
        <div
          class="placeholder"
          class:focused={selected === subject}
          class:dimmed={selected !== null && selected !== subject}
          style={`left:${box.left}px;top:${box.top}px;width:${box.width}px;height:${box.height}px;--subject-color:${colors[subject]};z-index:${selected === subject ? 30 : 20}`}
          onpointerdown={(e) => start(e, subject, "move")}
          onkeydown={(e) => key(e, subject)}
          role="button"
          tabindex="0"
          aria-label={`Move ${subjectLabels[subject]}`}
        >
          <div
            class="tab"
            class:below={side === "bottom"}
            class:inside={side.startsWith("inside")}
            class:insidebottom={side === "inside-bottom"}
          >
            <span>{subjectLabels[subject]}</span>
            <div class="buttons">
              {#if subject !== "bumblebee"}<button
                  aria-label={`${settings.editPreview[subject] ? "Hide" : "Show"} ${subjectLabels[subject]} preview`}
                  aria-pressed={settings.editPreview[subject]}
                  class:pressed={settings.editPreview[subject]}
                  onpointerdown={(e) => e.stopPropagation()}
                  onclick={() =>
                    void commit({
                      editPreview: {
                        [subject]: !settings.editPreview[subject],
                      },
                    }).catch(() => {})}
                  title="Show live preview">◉</button
                >{/if}
              <button
                aria-label={`Reset ${subjectLabels[subject]} position`}
                title="Reset position"
                onpointerdown={(e) => e.stopPropagation()}
                onclick={() => reset(subject, "position")}>↺</button
              >
              <button
                aria-label={`Reset ${subjectLabels[subject]} size`}
                title="Reset size"
                onpointerdown={(e) => e.stopPropagation()}
                onclick={() => reset(subject, "size")}>↔</button
              >
              <button
                aria-label={`${subjectLabels[subject]} options`}
                title="Settings"
                onpointerdown={(e) => e.stopPropagation()}
                onclick={() => {
                  selected = subject;
                  options = options === subject ? null : subject;
                }}>⚙</button
              >
            </div>
          </div>
          {#each subject === "puppet" ? ["nw", "ne"] : ["nw", "ne", "sw", "se"] as direction}<button
              class={`handle ${direction}`}
              aria-label={`Resize ${subjectLabels[subject]} ${direction}`}
              onpointerdown={(e) =>
                start(e, subject, direction as ResizeDirection)}
            ></button>{/each}
        </div>
      {/if}
    {/each}
    {#if settings.streamerVoiceBubble.enabled}
      {@const pose = streamerPose(settings, viewport)}
      {@const circle = subjectCircleForPose(pose)}
      <svg class="tail-guide" aria-hidden="true"
        ><circle
          cx={circle.center.x}
          cy={circle.center.y}
          r={circle.radius}
        /><circle
          class="person"
          cx={circle.center.x}
          cy={circle.center.y - circle.radius * 0.24}
          r={Math.max(5, circle.radius * 0.16)}
        /><path
          class="person"
          d={`M ${circle.center.x - circle.radius * 0.43} ${circle.center.y + circle.radius * 0.54} C ${circle.center.x - circle.radius * 0.43} ${circle.center.y + circle.radius * 0.08} ${circle.center.x - circle.radius * 0.21} ${circle.center.y - circle.radius * 0.02} ${circle.center.x} ${circle.center.y - circle.radius * 0.02} C ${circle.center.x + circle.radius * 0.21} ${circle.center.y - circle.radius * 0.02} ${circle.center.x + circle.radius * 0.43} ${circle.center.y + circle.radius * 0.08} ${circle.center.x + circle.radius * 0.43} ${circle.center.y + circle.radius * 0.54} Z`}
        /><line
          x1={pose.anchor.x}
          y1={pose.anchor.y}
          x2={pose.anchor.x + pose.tailVector.x}
          y2={pose.anchor.y + pose.tailVector.y}
        /></svg
      >
      <button
        class="subject-handle"
        style={`left:${circle.center.x - circle.radius}px;top:${circle.center.y - circle.radius}px;width:${circle.radius * 2}px;height:${circle.radius * 2}px`}
        aria-label="Move streamer camera target"
        title="Move camera target"
        onpointerdown={(e) => start(e, "streamerVoiceBubble", "subject")}
      ></button>
      <button
        class="radius-handle"
        style={`left:${circle.center.x + circle.radius}px;top:${circle.center.y}px`}
        aria-label="Resize streamer camera target"
        title="Resize camera target"
        onpointerdown={(e) => start(e, "streamerVoiceBubble", "radius")}
      ></button>
      <button
        class="tail-handle"
        style={`left:${pose.anchor.x + pose.tailVector.x}px;top:${pose.anchor.y + pose.tailVector.y}px`}
        aria-label="Drag streamer voice bubble tail"
        title="Drag tail target"
        onpointerdown={(e) => start(e, "streamerVoiceBubble", "tail")}
      ></button>
    {/if}
  {/if}
  {#if options}<div
      class="options-panel"
      role="dialog"
      aria-label={`${subjectLabels[options]} options`}
    >
      <header>
        <strong>{subjectLabels[options]}</strong><button
          aria-label="Close overlay options"
          onclick={() => (options = null)}>×</button
        >
      </header>
      <OverlayControls
        {settings}
        subject={options}
        {onPreview}
        onCommit={commit}
      />
    </div>{/if}
  {#if error}<p class="edit-error" role="alert">{error}</p>{/if}
</div>

<style>
  .overlay-editor {
    position: absolute;
    inset: 0;
    z-index: 10;
    overflow: hidden;
    pointer-events: none;
    touch-action: none;
    font: 600 12px system-ui;
  }
  .element-list {
    position: absolute;
    right: 12px;
    top: 10px;
    display: flex;
    gap: 5px;
    z-index: 50;
    pointer-events: auto;
    flex-wrap: wrap;
    justify-content: flex-end;
    max-width: 90%;
  }
  .element-list button {
    border: 1px solid #71819855;
    border-radius: 7px;
    padding: 5px 8px;
    background: #111827e8;
    color: #bdc6d6;
    font: 600 10px system-ui;
    cursor: pointer;
  }
  .element-list button.enabled {
    background: #e2e8f0e8;
    color: #334155;
  }
  .placeholder {
    position: absolute;
    border: 2px solid var(--subject-color);
    border-radius: 0 0 14px 14px;
    box-sizing: border-box;
    background: color-mix(in srgb, var(--subject-color) 6%, transparent);
    pointer-events: auto;
    cursor: grab;
    touch-action: none;
    outline: none;
  }
  .placeholder:active {
    cursor: grabbing;
  }
  .placeholder.focused {
    box-shadow: 0 0 0 2px #ffffff88;
  }
  .placeholder.dimmed {
    opacity: 0.45;
  }
  .tab {
    position: absolute;
    left: -2px;
    right: -2px;
    bottom: 100%;
    height: 30px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    padding: 2px 5px 2px 9px;
    background: var(--subject-color);
    border-radius: 10px 10px 0 0;
    color: #263044;
    white-space: nowrap;
  }
  .tab > span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tab.below {
    top: 100%;
    bottom: auto;
    border-radius: 0 0 10px 10px;
  }
  .tab.inside {
    top: 0;
    bottom: auto;
    border-radius: 0;
  }
  .tab.insidebottom {
    top: auto;
    bottom: 0;
  }
  .buttons {
    display: flex;
    gap: 2px;
    flex: none;
  }
  .buttons button {
    padding: 0;
    width: 23px;
    height: 23px;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    font-size: 15px;
    line-height: 1;
  }
  .buttons button:hover,
  .buttons button.pressed {
    background: #11182726;
  }
  .handle {
    position: absolute;
    width: 14px;
    height: 14px;
    padding: 0;
    background: var(--subject-color);
    border: 2px solid #fff;
    border-radius: 3px;
    z-index: 2;
  }
  .nw {
    left: -7px;
    top: -7px;
    cursor: nwse-resize;
  }
  .ne {
    right: -7px;
    top: -7px;
    cursor: nesw-resize;
  }
  .sw {
    left: -7px;
    bottom: -7px;
    cursor: nesw-resize;
  }
  .se {
    right: -7px;
    bottom: -7px;
    cursor: nwse-resize;
  }
  .tail-guide {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    z-index: 31;
  }
  .tail-guide line {
    stroke: #38bdf8;
    stroke-width: 2;
    stroke-dasharray: 5 4;
  }
  .tail-guide circle {
    stroke: #38bdf8;
    stroke-width: 2;
    stroke-dasharray: 5 4;
    fill: #38bdf815;
  }
  .tail-guide .person {
    fill: #38bdf860;
    stroke: none;
  }
  .subject-handle {
    position: absolute;
    border: 0;
    border-radius: 50%;
    padding: 0;
    background: transparent;
    color: #38bdf880;
    font-size: 50px;
    cursor: move;
    pointer-events: auto;
    z-index: 31;
  }
  .radius-handle {
    position: absolute;
    width: 16px;
    height: 16px;
    transform: translate(-50%, -50%);
    border: 2px solid white;
    border-radius: 50%;
    padding: 0;
    background: #38bdf8;
    cursor: ew-resize;
    pointer-events: auto;
    z-index: 33;
  }
  .tail-handle {
    position: absolute;
    width: 17px;
    height: 17px;
    transform: translate(-50%, -50%);
    border: 2px solid white;
    border-radius: 50%;
    padding: 0;
    background: #38bdf8;
    cursor: move;
    pointer-events: auto;
    z-index: 32;
  }
  .options-panel {
    position: absolute;
    z-index: 60;
    right: 16px;
    top: 50px;
    bottom: 16px;
    width: min(340px, calc(100% - 32px));
    box-sizing: border-box;
    overflow: auto;
    padding: 0 18px 16px;
    border-radius: 15px;
    background: var(--panel, #182024);
    box-shadow: 0 12px 36px #0008;
    pointer-events: auto;
  }
  .options-panel header {
    height: auto;
    padding: 12px 0;
    margin-bottom: 8px;
    position: sticky;
    top: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--panel, #182024);
    color: #e9ecec;
    border-bottom: 1px solid var(--border, #293439);
  }
  .options-panel header button {
    padding: 2px 8px;
    background: transparent;
    color: var(--muted, #95a2a5);
    border: 0;
    font-size: 24px;
    cursor: pointer;
  }
  .edit-error {
    position: absolute;
    bottom: 10px;
    left: 10px;
    right: 10px;
    padding: 10px;
    background: #fee2e2;
    color: #991b1b;
    border-radius: 8px;
    z-index: 70;
  }
</style>
