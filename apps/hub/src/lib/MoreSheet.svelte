<script lang="ts">
  import { Button, registerAdExclusion, t } from "@arena/ui";
  import { pushBack } from "./back";
  import Icon from "./Icon.svelte";
  import { MORE, type ScreenId } from "./nav";

  /**
   * The phone bottom bar's More sheet: a modal bottom sheet listing the remaining screens.
   * Focus is trapped inside while it is open. Esc, the backdrop or Close dismiss it (the caller
   * returns focus to More); picking a screen navigates and closes it (picking the current screen
   * just dismisses it).
   */
  let {
    id,
    current,
    onclose,
  }: {
    id: string;
    current: ScreenId | undefined;
    onclose: (reason: "dismiss" | "navigate") => void;
  } = $props();

  let panel: HTMLElement | undefined = $state();
  const titleId = $derived(id + "-title");

  const focusables = () =>
    panel
      ? [...panel.querySelectorAll<HTMLElement>("a[href],button:not([disabled])")].filter(
          (el) => el.offsetParent !== null,
        )
      : [];

  $effect(() => {
    focusables()[0]?.focus();
  });

  // An open sheet is an ad-exclusion surface (Story 4.13).
  $effect(() => registerAdExclusion("more-sheet"));

  // Android Back closes the sheet like Esc does.
  $effect(() => pushBack(() => onclose("dismiss")));

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onclose("dismiss");
      return;
    }
    if (event.key !== "Tab") return;
    const items = focusables();
    if (items.length === 0) return;
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    const inside = active instanceof HTMLElement && panel?.contains(active);
    if (event.shiftKey && (active === first || !inside)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !inside)) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="sheet-layer" data-testid="more-sheet-layer">
  <!-- The backdrop is a pointer convenience; Esc and Close are the keyboard routes. -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="backdrop" data-testid="more-sheet-backdrop" onclick={() => onclose("dismiss")}></div>
  <div
    class="sheet"
    {id}
    role="dialog"
    aria-modal="true"
    aria-labelledby={titleId}
    bind:this={panel}
  >
    <h2 class="type-hub-title" id={titleId}>{t("hub.more.title")}</h2>
    <ul>
      {#each MORE as screen (screen.id)}
        <li>
          <a
            class="row ui-interactive type-hub-body"
            href={screen.href}
            aria-current={current === screen.id ? "page" : undefined}
            onclick={() => onclose(current === screen.id ? "dismiss" : "navigate")}
          >
            <Icon name={screen.id} />
            <span>{t(screen.label)}</span>
          </a>
        </li>
      {/each}
    </ul>
    <div class="actions">
      <Button label="hub.more.close" variant="secondary" onclick={() => onclose("dismiss")} />
    </div>
  </div>
</div>

<style>
  .sheet-layer {
    position: fixed;
    /* Stops above the ad-banner slot, which no overlay covers (--ad-reserved is 0 without ads). */
    inset: 0 0 var(--ad-reserved);
    z-index: 10;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
  }

  .backdrop {
    position: absolute;
    inset: 0;
    background-color: color-mix(in srgb, var(--scrim-base) 72%, transparent);
  }

  /* HC: no translucency; the screen behind is covered by the pure black ground. */
  :global([data-theme="hc"]) .backdrop {
    background-color: var(--bg);
  }

  .sheet {
    --ring-gap: var(--surface);
    position: relative;
    max-height: calc(85dvh - var(--ad-reserved));
    overflow-y: auto;
    padding: var(--space-hub-panel-padding) var(--space-hub-gutter-phone);
    border-top: 1px solid var(--border-strong);
    border-radius: var(--radius-xl) var(--radius-xl) 0 0;
    background-color: var(--surface);
    color: var(--text);
  }

  h2 {
    margin: 0 0 var(--space-3);
  }

  ul {
    display: grid;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    --ui-bg: var(--surface);
    display: flex;
    align-items: center;
    gap: var(--space-4);
    min-height: var(--space-8);
    padding: 0 var(--space-3);
    border-radius: var(--radius-sm);
    color: var(--text);
    text-decoration: none;
  }

  .row[aria-current="page"] {
    --ui-bg: var(--hub-line);
    font-weight: var(--type-hub-title-weight);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    margin-top: var(--space-4);
  }
</style>
