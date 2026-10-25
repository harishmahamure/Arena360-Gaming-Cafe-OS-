<script lang="ts">
  import { t, type MessageKey } from "../messages/index.js";
  import Button from "./Button.svelte";
  import StatusBadge from "./StatusBadge.svelte";

  let {
    code,
    expiresAt,
    serverNow,
    onregenerate,
    regenerating = false,
  }: {
    code: string;
    expiresAt: number;
    serverNow?: number;
    onregenerate: () => void;
    regenerating?: boolean;
  } = $props();

  const MARKS = [600_000, 300_000, 60_000, 0];
  const id = $props.id();

  let now = $state(Date.now());
  let announcement = $state("");
  let status: MessageKey | null = $state(null);
  let codeEl: HTMLElement | undefined = $state();
  let controlsEl: HTMLElement | undefined = $state();
  let skew = $state(0);
  let passed = 0;
  let markedFor = Number.NaN;
  let wasExpired = false;
  let refocus = false;

  const remaining = $derived(Math.max(0, expiresAt - (now + skew)));
  const expired = $derived(remaining <= 0);
  const clock = $derived.by(() => {
    const s = Math.ceil(remaining / 1000);
    return [Math.floor(s / 60), s % 60].map((n) => String(n).padStart(2, "0")).join(":");
  });
  const groups = $derived([code.slice(0, 4), code.slice(4)]);

  const marksPassed = (ms: number) => MARKS.filter((m) => ms <= m).length;

  function markMessage(mark: number): string {
    if (mark === 0) return t("ui.pairing_code.expired_message");
    if (mark === 60_000) return t("ui.pairing_code.minute_left");
    return t("ui.pairing_code.minutes_left", { n: mark / 60_000 });
  }

  $effect.pre(() => {
    const clientNow = Date.now();
    skew = expiresAt && serverNow !== undefined ? serverNow - clientNow : 0;
    now = clientNow;
    status = null;
  });

  $effect.pre(() => {
    const isExpired = expired;
    if (isExpired && !wasExpired) {
      status = null;
      refocus = Boolean(controlsEl?.contains(document.activeElement));
    }
    wasExpired = isExpired;
  });

  $effect(() => {
    if (expired && refocus) {
      refocus = false;
      controlsEl?.querySelector("button")?.focus();
    }
  });

  function refresh() {
    now = Date.now();
  }

  $effect(() => {
    const n = marksPassed(remaining);
    if (expiresAt !== markedFor) {
      markedFor = expiresAt;
      passed = n;
      announcement = "";
      return;
    }
    if (n > passed) {
      passed = n;
      announcement = markMessage(MARKS[n - 1]);
    }
  });

  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  function selectCode() {
    if (!codeEl) return;
    codeEl.focus();
    const range = document.createRange();
    range.selectNodeContents(codeEl);
    const selection = window.getSelection();
    selection?.removeAllRanges();
    selection?.addRange(range);
  }

  async function copy() {
    status = null;
    try {
      await navigator.clipboard.writeText(code);
      status = "ui.pairing_code.copied";
    } catch {
      selectCode();
      status = "ui.pairing_code.copy_hint";
    }
  }
</script>

<svelte:window onfocus={refresh} />
<svelte:document onvisibilitychange={refresh} />

<section class="pairing-code" aria-labelledby="{id}-label" data-component="pairing-code">
  <div class="top">
    <h3 class="label" id="{id}-label">{t("ui.pairing_code.label")}</h3>
    <span class="single">{t("ui.pairing_code.single_use")}</span>
    {#if expired}
      <StatusBadge state="offline" label="ui.pairing_code.expired" />
    {/if}
  </div>
  <p
    class="value"
    class:expired
    tabindex="-1"
    bind:this={codeEl}
    data-testid="pairing-code-value"
    data-expired={expired ? "true" : undefined}
  >
    <span>{groups[0]}</span>
    <span>{groups[1]}</span>
  </p>
  <div class="controls" bind:this={controlsEl}>
    {#if expired}
      <p class="expired-message" data-testid="pairing-code-expired">
        {t("ui.pairing_code.expired_message")}
      </p>
      <div class="actions">
        <Button
          label="ui.pairing_code.regenerate"
          onclick={onregenerate}
          disabled={regenerating}
          disabledReason="ui.pairing_code.generating"
        />
      </div>
    {:else}
      <p class="expires" data-testid="pairing-code-expires">
        {t("ui.pairing_code.expires_in", { time: clock })}
      </p>
      <div class="actions">
        <Button label="ui.pairing_code.copy" variant="secondary" onclick={copy} />
        <Button
          label="ui.pairing_code.regenerate"
          variant="secondary"
          onclick={onregenerate}
          disabled={regenerating}
          disabledReason="ui.pairing_code.generating"
        />
      </div>
    {/if}
  </div>
  <p class="status" role="status" data-testid="pairing-code-status">
    {#if status}{t(status)}{/if}
  </p>
  <p class="visually-hidden" aria-live="polite" data-testid="pairing-code-announcement">
    {announcement}
  </p>
</section>

<style>
  .pairing-code {
    --status-badge-background: var(--surface);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    box-sizing: border-box;
    max-width: 100%;
    padding: var(--space-5);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background-color: var(--card-background);
    color: var(--text);
  }

  .top {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
  }

  .label {
    margin: 0;
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  .single {
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--line);
    border-radius: var(--radius-xs);
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
  }

  .value {
    display: flex;
    flex-wrap: wrap;
    gap: 0 0.5em;
    margin: 0;
    font-family: var(--type-web-display-family), system-ui, sans-serif;
    font-size: var(--type-web-display-size);
    font-weight: var(--type-web-display-weight);
    line-height: var(--type-web-display-line-height);
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.08em;
    user-select: all;
    overflow-wrap: anywhere;
  }

  .value:focus {
    outline: none;
  }

  .value:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .value.expired {
    color: var(--text-muted);
    text-decoration: line-through;
  }

  .expires,
  .expired-message,
  .status {
    margin: 0;
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
  }

  .expires {
    font-variant-numeric: tabular-nums;
  }

  .expired-message {
    font-weight: var(--type-hub-title-weight);
  }

  .controls {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
