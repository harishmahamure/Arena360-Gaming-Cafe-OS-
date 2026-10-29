<script lang="ts">
  import { onMount, tick } from "svelte";
  import { t } from "@arena/ui";
  import {
    currentStaffMessages,
    markMessagesRead,
    onStaffMessage,
    onStaffMessagesCleared,
  } from "./bridge";
  import { add, markRead, timeOf, unread, type Note } from "./messages";

  // The messages bell (Story 6.16): the session's staff messages, newest first with their time.
  // An unread message adds an `info` dot and a count, and the count is part of the accessible
  // name. Opening it marks every message read. It lives as long as the session screen does, so
  // the next player starts with an empty list.

  let open = $state(false);
  let notes: Note[] = $state([]);
  let root: HTMLDivElement | undefined = $state();
  let button: HTMLButtonElement | undefined = $state();
  let panel: HTMLDivElement | undefined = $state();

  const count = $derived(unread(notes));
  const label = $derived(
    count === 0
      ? t("player.messages.label")
      : count === 1
        ? t("player.messages.label_unread_one")
        : t("player.messages.label_unread", { count }),
  );

  async function toggle() {
    if (open) return close(false);
    open = true;
    await tick();
    panel?.focus();
  }

  /** Opens the list (the account menu's Messages row); one already open stays so. */
  export async function show() {
    if (!open) await toggle();
  }

  function close(refocus: boolean) {
    open = false;
    if (refocus) button?.focus();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && open) {
      event.preventDefault();
      event.stopPropagation();
      close(true);
    }
  }

  function onFocusout(event: FocusEvent) {
    const next = event.relatedTarget as Node | null;
    if (open && next && root && !root.contains(next)) open = false;
  }

  // While the list is on screen, what is in it has been seen.
  $effect(() => {
    if (open && count > 0) {
      notes = markRead(notes);
      void markMessagesRead();
    }
  });

  onMount(() => {
    const stops: Array<() => void> = [];
    let gone = false;
    const away = (e: PointerEvent) => {
      if (open && root && !root.contains(e.target as Node)) open = false;
    };
    window.addEventListener("pointerdown", away);
    stops.push(() => window.removeEventListener("pointerdown", away));
    void (async () => {
      const keep = (stop: () => void) => (gone ? stop() : stops.push(stop));
      keep(
        await onStaffMessage((m) => {
          notes = add(notes, m);
        }),
      );
      keep(await onStaffMessagesCleared(() => (notes = [])));
      for (const m of await currentStaffMessages()) notes = add(notes, m);
    })();
    return () => {
      gone = true;
      for (const stop of stops) stop();
    };
  });
</script>

<div
  class="messages-bell"
  data-testid="messages-bell-root"
  bind:this={root}
  onkeydown={onKeydown}
  onfocusout={onFocusout}
  role="presentation"
>
  <button
    bind:this={button}
    type="button"
    class="bell"
    aria-label={label}
    aria-expanded={open}
    aria-controls="messages-panel"
    data-unread={count}
    data-testid="messages-bell"
    onclick={toggle}
  >
    <svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false">
      <path
        d="M12 3a6 6 0 0 0-6 6v3.5L4 16v1h16v-1l-2-3.5V9a6 6 0 0 0-6-6zm-2 16a2 2 0 0 0 4 0h-4z"
        fill="currentColor"
      />
    </svg>
    {#if count > 0}
      <span class="unread" aria-hidden="true" data-testid="messages-bell-dot">
        <span class="dot"></span>
        <span class="num type-kiosk-label tabular-nums" data-testid="messages-bell-count"
          >{count}</span
        >
      </span>
    {/if}
  </button>

  {#if open}
    <div
      class="panel"
      id="messages-panel"
      role="region"
      aria-label={t("player.messages.title")}
      tabindex="-1"
      bind:this={panel}
      data-testid="messages-panel"
    >
      <p class="title type-kiosk-label">{t("player.messages.title")}</p>
      {#if notes.length === 0}
        <p class="empty type-kiosk-body" data-testid="messages-empty">
          {t("player.messages.empty")}
        </p>
      {:else}
        <ol data-testid="messages-list">
          {#each notes as note (note.id)}
            <li data-testid="message-item">
              <time class="type-kiosk-label tabular-nums" datetime={new Date(note.at).toISOString()}
                >{timeOf(note.at)}</time
              >
              <p class="type-kiosk-body" data-testid="message-text">{note.text}</p>
            </li>
          {/each}
        </ol>
      {/if}
    </div>
  {/if}
</div>

<style>
  .messages-bell {
    position: relative;
  }

  .bell {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    min-width: var(--space-touch-min);
    height: var(--space-touch-min);
    padding: 0;
    border: 0;
    border-radius: var(--radius-full);
    background-color: var(--messages-bell-background);
    color: var(--messages-bell-foreground);
    cursor: pointer;
  }

  .bell:has(.unread) {
    padding: 0 var(--space-3);
  }

  .bell:focus-visible,
  .bell:focus {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .unread {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: var(--radius-full);
    background-color: var(--messages-bell-unread-dot);
  }

  .panel {
    position: absolute;
    top: calc(100% + var(--space-2));
    right: 0;
    z-index: 5;
    display: grid;
    gap: var(--space-3);
    box-sizing: border-box;
    width: 360px;
    max-height: 60vh;
    padding: var(--space-4);
    overflow-y: auto;
    border: 1px solid var(--line);
    border-radius: var(--radius-xl);
    background-color: var(--messages-bell-background);
    color: var(--messages-bell-foreground);
    text-align: left;
  }

  .panel:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .panel p {
    margin: 0;
  }

  .title {
    color: var(--text-muted);
  }

  .empty {
    color: var(--text-muted);
  }

  ol {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    gap: var(--space-1);
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--line);
  }

  li:last-child {
    padding-bottom: 0;
    border-bottom: 0;
  }

  li time {
    color: var(--text-muted);
  }

  li p {
    overflow-wrap: anywhere;
  }
</style>
