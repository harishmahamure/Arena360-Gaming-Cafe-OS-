<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { PUBLIC_ANDROID_HUB_URL } from "$app/env/public";
  import { onDestroy, onMount, tick } from "svelte";
  import { invalidate } from "$app/navigation";
  import AccessPrompt from "../../lib/AccessPrompt.svelte";
  import { SNAPSHOT_DEPENDENCY } from "../../lib/snapshot";
  import { closeShellWindow, formatCountdown, pairHub } from "../../lib/pair";
  import type { LayoutData } from "../$types";

  let { data }: { data: LayoutData } = $props();

  const id = $props.id();
  const inputId = id + "-code";
  const hintId = id + "-hint";
  const errorId = id + "-error";
  const androidUrl = PUBLIC_ANDROID_HUB_URL || "https://arena360.cloud/download/android";

  let code = $state("");
  let busy = $state(false);
  let error: MessageKey | null = $state(null);
  let fieldError = $state(false);
  let notEntitled = $state(false);
  let pausedFor = $state(0);
  let announcement = $state("");
  let quitShown = $state(false);
  let input: HTMLInputElement | undefined = $state();
  let pausedUntil = 0;
  let timer: ReturnType<typeof setInterval> | undefined;

  // The Hub has its branch but lost the key that protected its sign-in: only a new code helps.
  const needsRepair = $derived(data.hub.ok && data.hub.snapshot.needs_repair === true);
  const title = $derived(needsRepair ? "hub.pair.repair_title" : "hub.pair.title");
  const paused = $derived(pausedFor > 0);
  const describedBy = $derived(
    [hintId, error || paused || notEntitled ? errorId : null].filter(Boolean).join(" "),
  );

  function tickPause() {
    pausedFor = Math.max(0, Math.ceil((pausedUntil - Date.now()) / 1000));
    if (pausedFor === 0) {
      clearInterval(timer);
      timer = undefined;
      announcement = "";
    }
  }

  function startPause(seconds: number) {
    clearInterval(timer);
    pausedUntil = Date.now() + seconds * 1000;
    tickPause();
    announcement = t("hub.pair.rate_limited", { time: formatCountdown(seconds) });
    timer = setInterval(tickPause, 250);
  }

  function show(message: MessageKey, invalidField = false) {
    error = message;
    fieldError = invalidField;
    announcement = t(message);
  }

  async function onsubmit(event: SubmitEvent) {
    event.preventDefault();
    if (busy || paused) return;
    error = null;
    fieldError = false;
    notEntitled = false;
    announcement = "";
    if (code.replace(/[\s-]/g, "") === "") {
      show("hub.pair.required", true);
      input?.focus();
      return;
    }
    busy = true;
    try {
      const result = await pairHub(code);
      if (result.kind === "paired") {
        announcement = t("hub.pair.paired");
        await invalidate(SNAPSHOT_DEPENDENCY);
        return;
      }
      if (result.kind === "invalid") {
        code = "";
        show("hub.pair.invalid", true);
      } else if (result.kind === "not_entitled") {
        notEntitled = true;
        announcement = t("hub.pair.not_entitled");
      } else if (result.kind === "rate_limited") {
        startPause(result.retryAfterS);
      } else if (result.kind === "cloud_unreachable") {
        show("hub.pair.unreachable");
      } else {
        show("hub.pair.hub_unreachable");
      }
    } finally {
      busy = false;
    }
    await tick();
    input?.focus();
  }

  // Pressing "Pair" must not take focus from the field: on Android that closes the keyboard.
  function keepFieldFocus(event: Event) {
    if (document.activeElement === input) event.preventDefault();
  }

  async function quit() {
    if (await closeShellWindow()) return;
    quitShown = true;
  }

  onMount(() => input?.focus());
  onDestroy(() => clearInterval(timer));
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t(title) })}</title>
</svelte:head>

<section class="pair" aria-labelledby="pair-title" data-testid="hub-pair">
  <h1 class="type-hub-numeric" id="pair-title" data-testid="pair-title">{t(title)}</h1>
  <p class="type-hub-body muted" data-testid="pair-intro">
    {t(needsRepair ? "hub.pair.repair_intro" : "hub.pair.intro")}
  </p>
  <AccessPrompt />

  <form class="form" {onsubmit} novalidate>
    <div class="field">
      <label class="label type-hub-label" for={inputId}>{t("hub.pair.code")}</label>
      <input
        class="ui-interactive input type-hub-title"
        class:invalid={fieldError}
        id={inputId}
        name="code"
        type="text"
        autocomplete="off"
        autocapitalize="characters"
        spellcheck="false"
        inputmode="text"
        bind:this={input}
        bind:value={code}
        aria-invalid={fieldError ? "true" : undefined}
        aria-describedby={describedBy}
        data-testid="pair-code"
      />
      <p class="hint type-hub-label" id={hintId}>{t("hub.pair.code_hint")}</p>
    </div>

    <div id={errorId} class="messages">
      {#if error}
        <p class="message type-hub-label" data-testid="pair-error">
          <span aria-hidden="true">⚠</span>
          <span>{t(error)}</span>
        </p>
      {/if}
      {#if notEntitled}
        <p class="message type-hub-label" data-testid="pair-not-entitled">
          <span aria-hidden="true">⚠</span>
          <span>{t("hub.pair.not_entitled")}</span>
        </p>
        <a
          class="link type-hub-label"
          href={androidUrl}
          target="_blank"
          rel="noopener"
          data-testid="pair-android-link">{t("hub.pair.android_download")}</a
        >
      {/if}
      {#if paused}
        <p class="message type-hub-label" role="timer" data-testid="pair-rate-limited">
          <span aria-hidden="true">⚠</span>
          <span>{t("hub.pair.rate_limited", { time: formatCountdown(pausedFor) })}</span>
        </p>
      {/if}
    </div>

    <div class="actions">
      <span
        class="press"
        role="presentation"
        onmousedown={keepFieldFocus}
        onpointerdown={keepFieldFocus}
      >
        <Button
          label="hub.pair.submit"
          type="submit"
          disabled={busy || paused}
          disabledReason={paused ? "hub.pair.paused" : "hub.pair.pairing"}
        />
      </span>
      <Button label="hub.pair.quit" variant="secondary" onclick={quit} />
    </div>
  </form>

  {#if quitShown}
    <p class="type-hub-body" role="status" data-testid="pair-closed">{t("hub.pair.closed")}</p>
  {/if}

  <p class="visually-hidden" aria-live="assertive" data-testid="pair-announce">
    {announcement}
  </p>
</section>

<style>
  .pair {
    display: grid;
    gap: var(--space-4);
    max-width: 32rem;
  }

  h1,
  p {
    margin: 0;
  }

  .muted,
  .hint {
    color: var(--text-muted);
  }

  .form {
    display: grid;
    gap: var(--space-4);
  }

  .field {
    display: grid;
    gap: var(--space-2);
  }

  .label {
    color: var(--text);
  }

  .input {
    --ui-bg: var(--text-field-background);
    box-sizing: border-box;
    width: 100%;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font-family: inherit;
    letter-spacing: 0.15em;
    text-transform: uppercase;
  }

  .input:focus-visible {
    border-width: 2px;
    border-color: var(--accent);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .input.invalid {
    border-width: 2px;
    border-color: var(--danger);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .messages {
    display: grid;
    gap: var(--space-2);
  }

  .messages:empty {
    display: none;
  }

  .message {
    display: flex;
    gap: var(--space-2);
    color: var(--text-field-error-text);
  }

  .link {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    color: var(--text);
    text-decoration: underline;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .press {
    display: contents;
  }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
