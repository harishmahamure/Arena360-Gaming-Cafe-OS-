<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { AttractView, KioskHubState, PlayerContext } from "@arena/ui/generated";
  import { onMount, tick, untrack } from "svelte";
  import { playerLogin } from "./bridge";
  import { countdownText, lockLeftS, recordOutcome } from "./login";

  /**
   * The login form (Story 6.8): username and password, "Log in", the two help links and the
   * footer. The password goes to the Kiosk service and nowhere else, and is cleared from the
   * field the moment it is sent.
   */
  let {
    view,
    hub,
    onplayer,
    onclose,
  }: {
    view: AttractView | null;
    /** The Hub link as the service reports it (`null` until it says). */
    hub: KioskHubState | null;
    /** A correct login: the player to hold for. */
    onplayer: (player: PlayerContext) => void;
    onclose: () => void;
  } = $props();

  const id = $props.id();
  const errorId = id + "-error";

  let username = $state("");
  let password = $state("");
  let shown = $state(false);
  /** A request is out. */
  let checking = $state(false);
  /** Why the last try did not log in (shown once, under the fields). */
  let problem: "rejected" | "busy" | "unreachable" | null = $state(null);
  let busyName = $state("");
  /** The help link last pressed: its line shows in place. */
  let note: "forgot" | "create" | null = $state(null);
  /** Whole seconds of lock left (0: open). */
  let left = $state(lockLeftS(performance.now()));

  /** What a screen reader hears when the lock starts (said once, not every second). */
  let lockSaid = $state("");

  let usernameEl: HTMLInputElement | undefined = $state();
  let passwordEl: HTMLInputElement | undefined = $state();

  const locked = $derived(left > 0);
  const connected = $derived(hub === "connected");
  const failed = $derived(problem === "rejected");
  const pc = $derived(view?.device_name || t("app.name"));
  const zone = $derived(view?.zone ?? "");
  const footer = $derived(
    zone
      ? t("player.login.footer", { pc, zone, version: __KIOSK_VERSION__ })
      : t("player.login.footer_no_zone", { pc, version: __KIOSK_VERSION__ }),
  );
  const message = $derived(
    problem === "rejected"
      ? t("player.login.failure")
      : problem === "busy"
        ? t("player.login.busy", { name: busyName })
        : problem === "unreachable"
          ? t("player.hub.reconnecting_retry")
          : "",
  );

  // The lock counts down in whole seconds and re-enables the form at zero.
  $effect(() => {
    if (!locked) return;
    const timer = setInterval(() => {
      left = lockLeftS(performance.now());
      if (left === 0) void tick().then(() => usernameEl?.focus());
    }, 250);
    return () => clearInterval(timer);
  });

  $effect(() => {
    lockSaid = locked
      ? t("player.login.locked", { countdown: countdownText(untrack(() => left)) })
      : "";
  });

  let gone = false;

  onMount(() => {
    // A lock still running when the panel opens again shows at once (the Hub keeps it too).
    left = lockLeftS(performance.now());
    if (left === 0) usernameEl?.focus();
    return () => (gone = true);
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (checking || locked || !connected) return;
    const name = username.trim();
    const secret = password;
    if (!name) return usernameEl?.focus();
    if (!secret) return passwordEl?.focus();
    // Sent, then gone: the field never keeps it, shown or not.
    password = "";
    shown = false;
    problem = null;
    note = null;
    checking = true;
    const outcome = await playerLogin(name, secret);
    checking = false;
    recordOutcome(outcome, performance.now());
    left = lockLeftS(performance.now());
    if (gone) return;
    if (outcome.kind === "ok") {
      username = "";
      onplayer(outcome.player);
      return;
    }
    if (left > 0) return; // locked: the countdown says it
    problem = outcome.kind === "locked" ? null : outcome.kind;
    if (outcome.kind === "busy") busyName = outcome.deviceName;
    await tick();
    usernameEl?.focus();
  }
</script>

<form class="login" data-testid="login-form" novalidate onsubmit={submit}>
  <h2 class="type-kiosk-title" id="login-title">{t("player.attract.login.title")}</h2>

  <div class="fields">
    <div class="field">
      <label class="type-kiosk-label" for="{id}-username">{t("player.login.username")}</label>
      <input
        bind:this={usernameEl}
        bind:value={username}
        class="input type-kiosk-body"
        class:invalid={failed}
        id="{id}-username"
        data-testid="login-username"
        type="text"
        name="username"
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        maxlength={64}
        disabled={locked}
        aria-invalid={failed ? "true" : undefined}
        aria-describedby={message ? errorId : undefined}
      />
    </div>
    <div class="field">
      <label class="type-kiosk-label" for="{id}-password">{t("player.login.password")}</label>
      <div class="row">
        <input
          bind:this={passwordEl}
          bind:value={password}
          class="input type-kiosk-body"
          class:invalid={failed}
          id="{id}-password"
          data-testid="login-password"
          type={shown ? "text" : "password"}
          name="password"
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
          maxlength={256}
          disabled={locked}
          aria-invalid={failed ? "true" : undefined}
          aria-describedby={message ? errorId : undefined}
        />
        <button
          type="button"
          class="toggle type-kiosk-label"
          data-testid="login-show"
          aria-controls="{id}-password"
          aria-pressed={shown ? "true" : "false"}
          disabled={locked}
          onclick={() => (shown = !shown)}
        >
          {t(shown ? "ui.password.hide" : "ui.password.show")}
        </button>
      </div>
    </div>
  </div>

  <p
    class="error type-kiosk-label"
    id={errorId}
    data-testid="login-error"
    role={message ? "alert" : undefined}
  >
    {#if message}<span aria-hidden="true">⚠</span> {message}{/if}
  </p>

  <div class="actions">
    {#if locked}
      <div class="tabular-nums" data-testid="login-locked">
        <Button
          type="submit"
          label="player.login.submit"
          disabled={true}
          disabledReason="player.login.locked"
          reasonParams={{ countdown: countdownText(left) }}
        />
      </div>
    {:else if !connected}
      <Button
        type="submit"
        label="player.login.submit"
        disabled={true}
        disabledReason="player.hub.reconnecting_retry"
      />
    {:else}
      <Button
        type="submit"
        label="player.login.submit"
        disabled={checking}
        disabledReason="player.login.checking"
      />
    {/if}
    <Button variant="secondary" label="player.attract.login.close" onclick={onclose} />
  </div>

  <div class="links">
    <button
      type="button"
      class="link type-kiosk-label"
      data-testid="login-forgot"
      onclick={() => (note = "forgot")}>{t("player.login.forgot")}</button
    >
    <button
      type="button"
      class="link type-kiosk-label"
      data-testid="login-create"
      onclick={() => (note = "create")}>{t("player.login.create")}</button
    >
  </div>
  <p class="note type-kiosk-body" data-testid="login-note" role="status">
    {#if note === "forgot"}{t("player.login.forgot_password")}{/if}
    {#if note === "create"}{t("player.login.create_hint")}{/if}
  </p>

  <!-- Said once when the lock starts; the visible countdown is not a live region. -->
  <p class="visually-hidden" role="status" data-testid="login-lock-status">
    {lockSaid}
  </p>

  <p class="footer type-kiosk-label" data-testid="login-footer">{footer}</p>
</form>

<style>
  .login {
    display: grid;
    gap: var(--space-4);
    justify-items: stretch;
    width: min(32rem, 80vw);
    text-align: start;
  }

  .login > * {
    margin: 0;
  }

  h2 {
    text-align: center;
  }

  .fields {
    display: grid;
    gap: var(--space-4);
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

  .toggle:focus-visible,
  .link:focus-visible {
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
    flex-wrap: wrap;
    gap: var(--space-4);
    align-items: flex-start;
  }

  .links {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-5);
  }

  .link {
    min-height: var(--space-touch-min);
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--text);
    text-decoration: underline;
    cursor: pointer;
  }

  .note {
    min-height: 1.5em;
    color: var(--text);
  }

  .footer {
    color: var(--text-muted);
    text-align: center;
    overflow-wrap: anywhere;
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
