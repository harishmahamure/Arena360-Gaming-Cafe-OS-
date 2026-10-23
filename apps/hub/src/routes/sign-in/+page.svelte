<script lang="ts">
  import { Button, TextField, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onDestroy } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, signIn } from "../../lib/auth";

  let login = $state("");
  let password = $state("");
  let busy = $state(false);
  /** The one failure message; it never says which field was wrong. */
  let error: MessageKey | null = $state(null);
  /** Seconds left while sign-in is paused after too many failures. */
  let lockedFor = $state(0);
  /** Announced once when a failure or a pause starts (the countdown itself is a timer). */
  let announcement = $state("");
  let lockedUntil = 0;
  let timer: ReturnType<typeof setInterval> | undefined;

  const locked = $derived(lockedFor > 0);

  function tickLock() {
    lockedFor = Math.max(0, Math.ceil((lockedUntil - Date.now()) / 1000));
    if (lockedFor === 0) {
      clearInterval(timer);
      timer = undefined;
      announcement = "";
    }
  }

  function startLock(seconds: number) {
    clearInterval(timer);
    lockedUntil = Date.now() + seconds * 1000;
    error = null;
    tickLock();
    announcement = t("hub.sign_in.locked", { seconds });
    timer = setInterval(tickLock, 250);
  }

  async function onsubmit(event: SubmitEvent) {
    event.preventDefault();
    if (busy || locked) return;
    busy = true;
    error = null;
    announcement = "";
    try {
      const result = await signIn(login.trim(), password);
      if (result.kind === "ok") {
        password = "";
        // The layout's gate navigates (to `redirectTo`, else the floor): one navigation only.
        await invalidate(SESSION_DEPENDENCY);
        return;
      }
      password = "";
      if (result.kind === "locked") {
        startLock(result.retryAfterS);
      } else {
        error = result.kind === "failed" ? "hub.sign_in.failed" : "hub.sign_in.unreachable";
        announcement = t(error);
      }
    } finally {
      busy = false;
    }
  }

  onDestroy(() => clearInterval(timer));
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.sign_in.title") })}</title>
</svelte:head>

<section class="sign-in" aria-labelledby="sign-in-title">
  <h1 class="type-hub-numeric" id="sign-in-title">{t("hub.sign_in.title")}</h1>
  <p class="type-hub-body muted">{t("hub.sign_in.intro")}</p>

  <form class="form" {onsubmit} novalidate>
    <TextField label="hub.sign_in.login" name="login" autocomplete="username" bind:value={login} />
    <TextField
      label="hub.sign_in.password"
      name="password"
      type="password"
      autocomplete="current-password"
      bind:value={password}
    />

    {#if error}
      <p class="message type-hub-label" data-testid="sign-in-error">
        <span aria-hidden="true">⚠</span>
        <span>{t(error)}</span>
      </p>
    {/if}
    {#if locked}
      <p class="message type-hub-label" role="timer" data-testid="sign-in-locked">
        <span aria-hidden="true">⚠</span>
        <span>{t("hub.sign_in.locked", { seconds: lockedFor })}</span>
      </p>
    {/if}

    <Button
      label="hub.sign_in.submit"
      type="submit"
      disabled={busy || locked}
      disabledReason={locked ? "hub.sign_in.paused" : "hub.sign_in.signing_in"}
    />
  </form>
  <p class="visually-hidden" aria-live="assertive" data-testid="sign-in-announce">
    {announcement}
  </p>
</section>

<style>
  .sign-in {
    display: grid;
    gap: var(--space-4);
    max-width: 28rem;
  }

  h1,
  p {
    margin: 0;
  }

  .muted {
    color: var(--text-muted);
  }

  .form {
    display: grid;
    gap: var(--space-4);
  }

  .message {
    display: flex;
    gap: var(--space-2);
    color: var(--text-field-error-text);
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
