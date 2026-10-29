<script lang="ts">
  import { Button, t } from "@arena/ui";
  import { onMount, tick, untrack } from "svelte";
  import { recordUnlock, unlockLeftS } from "./account";
  import { playerUnlock } from "./bridge";
  import { countdownText } from "./login";

  // The player's own lock screen (Story 6.17): the password the player signed in with opens it
  // (paste is allowed); staff can also Unlock from the tile sheet. The password goes to the Kiosk
  // service and nowhere else and is cleared from the field the moment it is sent. Five wrong
  // passwords in a row pause the field for 60 s, with the countdown on the button.

  let { connected }: { connected: boolean } = $props();

  const id = $props.id();
  const errorId = id + "-error";

  let password = $state("");
  let shown = $state(false);
  let checking = $state(false);
  let problem: "rejected" | "unreachable" | null = $state(null);
  /** Whole seconds of pause left (0: open). */
  let left = $state(unlockLeftS(performance.now()));
  let input: HTMLInputElement | undefined = $state();
  let said = $state("");

  const paused = $derived(left > 0);
  const message = $derived(
    problem === "rejected"
      ? t("player.unlock.failure")
      : problem === "unreachable"
        ? t("player.hub.reconnecting_retry")
        : "",
  );

  $effect(() => {
    if (!paused) return;
    const timer = setInterval(() => {
      left = unlockLeftS(performance.now());
      if (left === 0) void tick().then(() => input?.focus());
    }, 250);
    return () => clearInterval(timer);
  });

  // Said once when the pause starts; the visible countdown is not a live region.
  $effect(() => {
    said = paused
      ? t("player.unlock.locked", { countdown: countdownText(untrack(() => left)) })
      : "";
  });

  let gone = false;

  onMount(() => {
    left = unlockLeftS(performance.now());
    if (left === 0) input?.focus();
    return () => (gone = true);
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (checking || paused || !connected) return;
    const secret = password;
    if (!secret) return input?.focus();
    // Sent, then gone: the field never keeps it, shown or not.
    password = "";
    shown = false;
    problem = null;
    checking = true;
    const outcome = await playerUnlock(secret);
    checking = false;
    recordUnlock(outcome, performance.now());
    left = unlockLeftS(performance.now());
    if (gone || outcome.kind === "ok") return;
    if (left === 0) problem = outcome.kind === "locked" ? null : outcome.kind;
    await tick();
    input?.focus();
  }
</script>

<form class="unlock" data-testid="unlock-form" novalidate onsubmit={submit}>
  <p class="hint type-kiosk-body">{t("player.lock.by_player_hint")}</p>
  <div class="field">
    <label class="type-kiosk-label" for="{id}-password">{t("player.unlock.password")}</label>
    <div class="row">
      <input
        bind:this={input}
        bind:value={password}
        class="input type-kiosk-body"
        class:invalid={problem === "rejected"}
        id="{id}-password"
        data-testid="unlock-password"
        type={shown ? "text" : "password"}
        name="password"
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        maxlength={256}
        disabled={paused}
        aria-invalid={problem === "rejected" ? "true" : undefined}
        aria-describedby={message ? errorId : undefined}
      />
      <button
        type="button"
        class="toggle type-kiosk-label"
        data-testid="unlock-show"
        aria-controls="{id}-password"
        aria-pressed={shown ? "true" : "false"}
        disabled={paused}
        onclick={() => (shown = !shown)}
      >
        {t(shown ? "ui.password.hide" : "ui.password.show")}
      </button>
    </div>
  </div>
  <p
    class="error type-kiosk-label"
    id={errorId}
    data-testid="unlock-error"
    role={message ? "alert" : undefined}
  >
    {#if message}<span aria-hidden="true">⚠</span> {message}{/if}
  </p>
  <div class="actions">
    {#if paused}
      <div class="tabular-nums" data-testid="unlock-paused">
        <Button
          type="submit"
          label="player.unlock.submit"
          disabled={true}
          disabledReason="player.unlock.locked"
          reasonParams={{ countdown: countdownText(left) }}
        />
      </div>
    {:else if !connected}
      <Button
        type="submit"
        label="player.unlock.submit"
        disabled={true}
        disabledReason="player.hub.reconnecting_retry"
      />
    {:else}
      <Button
        type="submit"
        label="player.unlock.submit"
        disabled={checking}
        disabledReason="player.unlock.checking"
      />
    {/if}
  </div>
  <p class="visually-hidden" role="status" data-testid="unlock-lock-status">{said}</p>
</form>

<style>
  .unlock {
    display: grid;
    gap: var(--space-4);
    justify-items: stretch;
    width: min(28rem, 80vw);
    margin-top: var(--space-5);
    text-align: start;
  }

  .unlock > * {
    margin: 0;
  }

  .hint {
    color: var(--text-muted);
    text-align: center;
  }

  .field {
    display: grid;
    gap: var(--space-2);
    min-width: 0;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .input {
    flex: 1 1 12em;
    min-width: 0;
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--text-field-background);
    color: var(--text-field-foreground);
    font: inherit;
  }

  .input:focus-visible {
    border-width: 2px;
    border-color: var(--accent);
    padding: 0 calc(var(--space-3) - 1px);
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .input.invalid {
    border-width: 2px;
    border-color: var(--danger);
    padding: 0 calc(var(--space-3) - 1px);
  }

  .input:disabled {
    cursor: not-allowed;
    opacity: 0.6;
  }

  .toggle {
    min-width: var(--space-touch-min);
    min-height: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--button-secondary-background);
    color: var(--button-secondary-foreground);
    cursor: pointer;
  }

  .toggle:focus-visible {
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .error {
    min-height: 1.5em;
    color: var(--text-field-error-text);
    font-weight: 600;
  }

  .actions {
    display: flex;
    justify-content: center;
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
