<script lang="ts">
  import type { Snippet } from "svelte";
  import { t, type MessageKey } from "../messages/index.js";
  import { STATUS_LABELS, type Status } from "../status.js";
  import AlertBadge, { ALERT_TAGS, type AlertTag } from "./AlertBadge.svelte";
  import GroupChip from "./GroupChip.svelte";
  import StatusBadge from "./StatusBadge.svelte";

  let {
    name,
    state,
    label,
    time,
    warning = false,
    warningText,
    console: isConsole = false,
    secondary,
    group,
    alerts,
    alertTag,
    note,
    tag = "div",
    tabindex = 0,
    onactivate,
    children,
    ...rest
  }: {
    name: string;
    state: Status;
    label?: MessageKey;
    time?: string;
    warning?: boolean;
    warningText?: string;
    console?: boolean;
    secondary?: Snippet | string;
    group?: { name: string; colour: string };
    alerts?: number;
    alertTag?: AlertTag;
    note?: string;
    tag?: string;
    tabindex?: number;
    onactivate?: () => void;
    children?: Snippet;
    [key: string]: unknown;
  } = $props();

  const INNER = "button, input, a"; // i18n-ignore
  const unassigned = $derived(state === "unassigned");
  const accessibleName = $derived(
    [
      name,
      t(label ?? STATUS_LABELS[state]),
      alertTag ? t(ALERT_TAGS[alertTag]) : "",
      note,
      time,
      warning ? (warningText ?? t("ui.floor_tile.time_warning")) : "",
    ]
      .filter(Boolean)
      .join(", "),
  );
  const activate = (e: Event) => {
    if (e.target === e.currentTarget) onactivate?.();
  };
</script>

<svelte:element
  this={tag}
  class="tile state-{state}"
  class:warning
  data-variant={isConsole ? "console" : undefined}
  data-component="floor-tile"
  {...rest}
>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="face"
    data-tile-face
    role="group"
    {tabindex}
    aria-label={accessibleName}
    onclick={(e) => !(e.target as Element).closest(INNER) && onactivate?.()}
    onkeydown={(e) => e.key === "Enter" && activate(e)}
  >
    <div class="top">
      <span class="name type-hub-title"
        >{#if isConsole}<svg
            class="pad"
            viewBox="0 0 24 24"
            width="20"
            height="20"
            aria-hidden="true"
            focusable="false"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            data-testid="console-glyph"
            ><path
              d="M6 11h4M8 9v4M15 12h.01M18 10h.01M17.3 5H6.7a4 4 0 0 0-3.9 3.1l-1.7 7.4a3 3 0 0 0 5.2 2.4L8 16h8l1.2 1.9a3 3 0 0 0 5.2-2.4l-1.7-7.4A4 4 0 0 0 17.3 5z"
            /></svg
          >{/if}{name}</span
      >
      <span data-testid={unassigned ? undefined : "device-presence"}
        ><StatusBadge {state} {label} /></span
      >
      {#if alertTag}<AlertBadge tag={alertTag} />{:else if alerts !== undefined}<AlertBadge
          count={alerts}
        />{/if}
    </div>
    <span class="time type-hub-numeric tabular-nums" aria-hidden={time ? undefined : "true"}
      >{#if warning}<span class="glyph" aria-hidden="true" data-testid="tile-time-warning">⚠</span
        >{/if}{time ?? "—"}{#if warning && warningText}<span
          class="warning-text type-hub-label"
          data-testid="tile-warning-text">{warningText}</span
        >{/if}</span
    >
    <div class="bottom">
      <span class="secondary type-hub-label">
        {#if typeof secondary === "string"}{secondary}{:else if secondary}{@render secondary()}{/if}
      </span>
      {#if group && !unassigned}<GroupChip name={group.name} colour={group.colour} />{/if}
    </div>
    {@render children?.()}
    {#if !unassigned}<span class="bar" aria-hidden="true"></span>{/if}
  </div>
</svelte:element>

<style>
  .tile {
    box-sizing: border-box;
    min-height: 128px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--floor-tile-background);
    color: var(--text);
    list-style: none;
    overflow: hidden;
  }

  .tile.warning {
    border: 2px solid var(--warning);
  }

  .pad {
    flex: none;
    margin-inline-end: var(--space-1);
    vertical-align: text-bottom;
  }

  .warning-text {
    margin-inline-start: var(--space-2);
    color: var(--warning);
  }

  .glyph {
    margin-inline-end: var(--space-1);
    color: var(--warning);
  }

  .tile[data-transfer-target],
  .tile[data-group-target] {
    outline: 3px dashed var(--accent);
    outline-offset: -3px;
  }

  .tile.state-unassigned {
    border: 2px dashed var(--state-unassigned);
  }

  .face {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    box-sizing: border-box;
    min-height: inherit;
    padding: var(--space-hub-tile-padding);
    padding-bottom: calc(var(--space-hub-tile-padding) + 3px);
  }

  .top,
  .bottom {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .name {
    flex: 1 1 auto;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .time {
    flex: 1 0 auto;
  }

  .bottom {
    justify-content: space-between;
    align-items: flex-end;
  }

  .secondary {
    display: grid;
    flex: 1 1 auto;
    min-width: 0;
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }

  .bottom :global([data-component="group-chip"]) {
    margin-inline-start: auto;
  }

  .bar {
    position: absolute;
    inset: auto 0 0;
    height: 3px;
    background-color: var(--state-free);
  }

  .state-in-session .bar {
    background-color: var(--state-in-session);
  }
  .state-paused .bar {
    background-color: var(--state-paused);
  }
  .state-reserved .bar {
    background-color: var(--state-reserved);
  }
  .state-order-pending .bar {
    background-color: var(--state-order-pending);
  }
  .state-maintenance .bar {
    background-color: var(--state-maintenance);
  }
  .state-offline .bar {
    background-color: var(--state-offline);
  }
</style>
