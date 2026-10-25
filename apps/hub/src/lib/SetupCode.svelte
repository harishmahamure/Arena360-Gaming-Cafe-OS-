<script lang="ts">
  import { Button, PasswordField, TextField, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onDestroy } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, useSetupCode } from "./auth";

  let { onback }: { onback: () => void } = $props();

  let step = $state<"code" | "choose">("code");
  let login = $state("");
  let code = $state("");
  let password = $state("");
  let pin = $state("");
  let pinAgain = $state("");
  let busy = $state(false);
  let error: MessageKey | null = $state(null);
  let field: "code" | "password" | "pin" | "pin_again" | null = $state(null);
  let lockedFor = $state(0);
  let announcement = $state("");
  let lockedUntil = 0;
  let timer: ReturnType<typeof setInterval> | undefined;

  const locked = $derived(lockedFor > 0);
  const PIN_SHAPE = /^[0-9]{4,6}$/;
  const CODE_SHAPE = /^[A-Za-z0-9]{8}$/;

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

  function fail(key: MessageKey, which: typeof field) {
    error = key;
    field = which;
    announcement = t(key);
  }

  function next(event: SubmitEvent) {
    event.preventDefault();
    error = null;
    field = null;
    if (!CODE_SHAPE.test(code.replace(/[\s_-]/g, ""))) return fail("hub.setup.code_shape", "code");
    step = "choose";
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (busy || locked) return;
    error = null;
    field = null;
    announcement = "";
    if (password.length < 10 || password.length > 128) {
      return fail("hub.setup.password_short", "password");
    }
    if (!PIN_SHAPE.test(pin)) return fail("hub.setup.pin_shape", "pin");
    if (pin !== pinAgain) return fail("hub.setup.pin_mismatch", "pin_again");
    busy = true;
    try {
      const result = await useSetupCode(login.trim(), code, password, pin);
      if (result.kind === "ok") {
        password = pin = pinAgain = code = "";
        await invalidate(SESSION_DEPENDENCY);
        return;
      }
      if (result.kind === "locked") {
        step = "code";
        startLock(result.retryAfterS);
      } else if (result.kind === "invalid") {
        step = "code";
        fail("hub.setup.invalid", "code");
      } else if (result.kind === "weak") {
        fail(
          result.field === "pin" ? "hub.setup.pin_shape" : "hub.setup.password_short",
          result.field,
        );
      } else {
        fail("hub.sign_in.unreachable", null);
      }
      code = "";
    } finally {
      busy = false;
    }
  }

  onDestroy(() => clearInterval(timer));
</script>

<section class="setup" aria-labelledby="setup-title">
  <h1 class="type-hub-numeric" id="setup-title">{t("hub.setup.title")}</h1>
  <p class="type-hub-body muted">{t("hub.setup.intro")}</p>

  {#if step === "code"}
    <form class="form" onsubmit={next} novalidate>
      <TextField
        label="hub.setup.login"
        name="login"
        type="tel"
        autocomplete="username"
        bind:value={login}
      />
      <TextField
        label="hub.setup.code"
        name="setup-code"
        autocomplete="one-time-code"
        bind:value={code}
        error={field === "code" ? error : null}
      />
      <p class="type-hub-label muted">{t("hub.setup.code_hint")}</p>
      {#if locked}
        <p class="message type-hub-label" role="timer" data-testid="setup-locked">
          <span aria-hidden="true">⚠</span>
          <span>{t("hub.sign_in.locked", { seconds: lockedFor })}</span>
        </p>
      {/if}
      <Button
        label="hub.setup.continue"
        type="submit"
        disabled={locked}
        disabledReason="hub.sign_in.paused"
      />
      <Button label="hub.setup.back" variant="secondary" onclick={onback} />
    </form>
  {:else}
    <form class="form" onsubmit={save} novalidate>
      <PasswordField
        label="hub.setup.password"
        name="new-password"
        hint="hub.setup.password_rule"
        bind:value={password}
        error={field === "password" ? error : null}
      />
      <TextField
        label="hub.setup.pin"
        name="new-pin"
        type="password"
        inputmode="numeric"
        autocomplete="off"
        bind:value={pin}
        error={field === "pin" ? error : null}
      />
      <TextField
        label="hub.setup.pin_again"
        name="new-pin-again"
        type="password"
        inputmode="numeric"
        autocomplete="off"
        bind:value={pinAgain}
        error={field === "pin_again" ? error : null}
      />
      {#if error && field === null}
        <p class="message type-hub-label" data-testid="setup-error">
          <span aria-hidden="true">⚠</span>
          <span>{t(error)}</span>
        </p>
      {/if}
      <Button
        label="hub.setup.submit"
        type="submit"
        disabled={busy}
        disabledReason="hub.setup.saving"
      />
    </form>
  {/if}
  <p class="visually-hidden" aria-live="assertive" data-testid="setup-announce">
    {announcement}
  </p>
</section>

<style>
  .setup {
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
