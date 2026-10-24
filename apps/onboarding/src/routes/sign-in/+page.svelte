<script lang="ts">
  import { Button, CodeField, PasswordField, TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { SigninMethod } from "@arena/ui/generated";
  import { goto } from "$app/navigation";
  import { onDestroy, tick } from "svelte";
  import { sendSigninSms, signIn, verifySignin, type ApiFailure } from "../../lib/api";
  import { minSec } from "../../lib/validate";
  import { normalizeBackupCode } from "../../lib/signin";

  type Notice = { key: MessageKey; params?: Record<string, string | number> };

  const LOW_CODES = 3;
  const MAX_ATTEMPTS = 5;
  const CHALLENGE_MS = 5 * 60 * 1000;

  let step: "form" | "challenge" | "low" = $state("form");
  let heading: HTMLHeadingElement | undefined = $state();

  let login = $state("");
  let password = $state("");
  let loginError: MessageKey | null = $state(null);
  let passwordError: MessageKey | null = $state(null);
  let notice: Notice | null = $state(null);
  let sending = $state(false);
  let formEl: HTMLFormElement | undefined = $state();

  let challengeId = $state("");
  let methods: SigninMethod[] = $state([]);
  let mode: SigninMethod = $state("totp");
  let code = $state("");
  let backupCode = $state("");
  let codeError: MessageKey | null = $state(null);
  let status: Notice | null = $state(null);
  let checking = $state(false);
  let trust = $state(false);
  let codeField: { focus: () => void } | undefined = $state();
  let backupForm: HTMLFormElement | undefined = $state();
  let remaining = $state(0);

  let resendAt = $state(0);
  let now = $state(Date.now());
  let resendAnnouncement: Notice | null = $state(null);
  let smsBusy = $state(false);
  let timer: ReturnType<typeof setInterval> | null = null;
  let attempts = 0;
  let expiresAt = 0;
  let dead = $state(false);
  let expiry: ReturnType<typeof setTimeout> | null = null;

  function stopExpiry() {
    if (expiry !== null) clearTimeout(expiry);
    expiry = null;
  }

  function expireAt(at: number) {
    stopExpiry();
    expiresAt = at;
    expiry = setTimeout(endChallenge, Math.max(0, at - Date.now()));
  }

  async function endChallenge() {
    stopExpiry();
    stopTimer();
    dead = true;
    codeError = null;
    status = null;
    notice = null;
    await tick();
    heading?.focus();
  }

  const waitS = $derived(Math.max(0, Math.ceil((resendAt - now) / 1000)));
  const canResend = $derived(waitS === 0 && !smsBusy && !checking);
  const lowTitle = $derived<Notice>(
    remaining === 0
      ? { key: "onboarding.signin.low_title_zero" }
      : remaining === 1
        ? { key: "onboarding.signin.low_title_one" }
        : { key: "onboarding.signin.low_title", params: { n: remaining } },
  );

  function stopTimer() {
    if (timer !== null) clearInterval(timer);
    timer = null;
  }

  function countdown(seconds: number) {
    stopTimer();
    now = Date.now();
    resendAt = now + seconds * 1000;
    resendAnnouncement =
      seconds > 0 ? { key: "onboarding.code.resend_in", params: { time: minSec(seconds) } } : null;
    if (seconds <= 0) return;
    timer = setInterval(() => {
      now = Date.now();
      if (now >= resendAt) {
        stopTimer();
        resendAnnouncement = { key: "onboarding.code.resend_ready" };
      }
    }, 250);
  }

  onDestroy(() => {
    stopTimer();
    stopExpiry();
  });

  async function show(next: typeof step) {
    step = next;
    await tick();
    heading?.focus();
  }

  function failure(f: ApiFailure): Notice {
    if (f.error.code === "AUTH_LOCKED") {
      return { key: "onboarding.signin.locked", params: { time: minSec(f.retryAfterS ?? 900) } };
    }
    if (f.error.code === "RATE_LIMITED") {
      return {
        key: "onboarding.error.rate_limited",
        params: { time: minSec(f.retryAfterS ?? 60) },
      };
    }
    if (f.error.code === "AUTH_FAILED") return { key: "onboarding.signin.failed" };
    return { key: f.offline ? "onboarding.error.offline" : "onboarding.error.generic" };
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (sending) return;
    notice = null;
    loginError = login.trim() === "" ? "onboarding.signin.login_required" : null;
    passwordError = password === "" ? "onboarding.signin.password_required" : null;
    if (loginError || passwordError) {
      await tick();
      focusFirstInvalid(formEl);
      return;
    }
    sending = true;
    const res = await signIn(login.trim(), password);
    sending = false;
    if (!res.ok) {
      if (res.error.code === "VALIDATION_FAILED") {
        if (res.error.field === "password") passwordError = "onboarding.signin.password_required";
        else loginError = "onboarding.signin.login_required";
        await tick();
        focusFirstInvalid(formEl);
        return;
      }
      notice = failure(res);
      return;
    }
    password = "";
    const reply = res.data;
    if (reply.next) {
      await goto("/account-setup");
      return;
    }
    if (!reply.challenge_id) {
      notice = { key: "onboarding.error.generic" };
      return;
    }
    challengeId = reply.challenge_id;
    methods = reply.methods ?? ["totp", "backup_code"];
    trust = false;
    attempts = 0;
    dead = false;
    expireAt(Date.now() + CHALLENGE_MS);
    await useMethod("totp", false);
    await show("challenge");
    codeField?.focus();
  }

  async function useMethod(next: SigninMethod, focus = true) {
    mode = next;
    code = "";
    backupCode = "";
    codeError = null;
    status = null;
    notice = null;
    if (!focus) return;
    await tick();
    if (next === "backup_code") backupForm?.querySelector("input")?.focus();
    else codeField?.focus();
  }

  async function textMe() {
    if (dead || smsBusy) return;
    if (!canResend) {
      notice = { key: "onboarding.code.resend_in", params: { time: minSec(waitS) } };
      return;
    }
    smsBusy = true;
    notice = null;
    codeError = null;
    const res = await sendSigninSms(challengeId);
    smsBusy = false;
    if (res.ok) {
      const resent = mode === "sms";
      await useMethod("sms");
      status = resent ? { key: "onboarding.code.resent" } : null;
      countdown(res.data.resend_after_s);
      expireAt(Math.max(expiresAt, Date.now() + CHALLENGE_MS));
      return;
    }
    if (res.error.code === "RATE_LIMITED") countdown(res.retryAfterS ?? 30);
    notice = failure(res);
  }

  async function verify(method: SigninMethod, value: string) {
    if (checking || dead) return;
    if (Date.now() >= expiresAt) {
      await endChallenge();
      return;
    }
    checking = true;
    codeError = null;
    notice = null;
    status = { key: "onboarding.signin.checking" };
    const res = await verifySignin(challengeId, method, value, trust);
    checking = false;
    status = null;
    if (res.ok) {
      stopTimer();
      const left = res.data.backup_codes_remaining;
      if (method === "backup_code" && left !== undefined && left <= LOW_CODES) {
        remaining = left;
        await show("low");
        return;
      }
      await goto("/account-setup");
      return;
    }
    code = "";
    if (res.error.code === "AUTH_FAILED") {
      attempts += 1;
      if (attempts >= MAX_ATTEMPTS) {
        await endChallenge();
        return;
      }
      codeError = "onboarding.signin.failed";
    } else {
      notice = failure(res);
    }
    await tick();
    if (method === "backup_code") backupForm?.querySelector("input")?.focus();
    else codeField?.focus();
  }

  async function submitBackup(event: SubmitEvent) {
    event.preventDefault();
    const normalized = normalizeBackupCode(backupCode);
    if (!normalized) {
      codeError = "onboarding.signin.backup_required";
      await tick();
      focusFirstInvalid(backupForm);
      return;
    }
    await verify("backup_code", normalized);
  }

  async function startOver() {
    stopTimer();
    stopExpiry();
    dead = false;
    challengeId = "";
    code = "";
    backupCode = "";
    codeError = null;
    status = null;
    notice = null;
    await show("form");
  }
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("onboarding.signin.title") })}</title>
</svelte:head>

{#if step === "form"}
  <h1 bind:this={heading} tabindex="-1">{t("onboarding.signin.title")}</h1>
  <p class="intro">{t("onboarding.signin.intro")}</p>
  <form class="form" bind:this={formEl} novalidate onsubmit={submit}>
    <TextField
      label="onboarding.signin.login"
      name="username"
      autocomplete="username"
      required
      bind:value={login}
      error={loginError}
    />
    <PasswordField
      label="onboarding.password.label"
      name="password"
      autocomplete="current-password"
      required
      bind:value={password}
      error={passwordError}
    />
    <p class="notice" role="alert">
      {#if notice}<span class="glyph" aria-hidden="true">⚠</span>
        <span>{t(notice.key, notice.params)}</span>{/if}
    </p>
    <div class="actions">
      <Button
        label="onboarding.signin.submit"
        type="submit"
        disabled={sending}
        disabledReason="onboarding.signin.signing_in"
      />
    </div>
  </form>
  <p class="aside">
    {t("onboarding.signin.new_here")}
    <a class="link" href="/">{t("onboarding.signin.get_started")}</a>
  </p>
{:else if step === "challenge"}
  <h1 bind:this={heading} tabindex="-1">{t("onboarding.signin.challenge_title")}</h1>
  {#if dead}
    <p class="intro" role="alert">{t("onboarding.signin.dead")}</p>
    <div class="actions">
      <Button label="onboarding.signin.again" onclick={startOver} />
    </div>
  {:else}
    {#if mode === "totp"}
      <p class="intro">{t("onboarding.signin.totp_intro")}</p>
      <CodeField
        bind:this={codeField}
        bind:value={code}
        label="onboarding.totp.code_label"
        error={codeError}
        busy={checking}
        autofocus={false}
        oncomplete={(v) => verify("totp", v)}
      />
    {:else if mode === "sms"}
      <p class="intro">{t("onboarding.signin.sms_intro")}</p>
      <CodeField
        bind:this={codeField}
        bind:value={code}
        label="onboarding.signin.sms_label"
        error={codeError}
        busy={checking}
        autofocus={false}
        oncomplete={(v) => verify("sms", v)}
      />
      <div class="actions">
        <Button
          label="onboarding.code.resend"
          variant="secondary"
          onclick={textMe}
          disabled={!canResend}
          disabledReason="onboarding.code.resend_wait"
        />
      </div>
      {#if waitS > 0}
        <p class="countdown" data-testid="resend-countdown">
          {t("onboarding.code.resend_in", { time: minSec(waitS) })}
        </p>
      {/if}
      <p class="visually-hidden" aria-live="polite" data-testid="resend-announcement">
        {#if resendAnnouncement}{t(resendAnnouncement.key, resendAnnouncement.params)}{/if}
      </p>
    {:else}
      <p class="intro">{t("onboarding.signin.backup_intro")}</p>
      <form class="form" bind:this={backupForm} novalidate onsubmit={submitBackup}>
        <TextField
          label="onboarding.signin.backup_label"
          name="backup_code"
          autocomplete="one-time-code"
          required
          bind:value={backupCode}
          error={codeError}
        />
        <div class="actions">
          <Button
            label="onboarding.signin.submit"
            type="submit"
            disabled={checking}
            disabledReason="onboarding.signin.checking"
          />
        </div>
      </form>
    {/if}

    <p class="status" aria-live="polite">
      {#if status}{t(status.key, status.params)}{/if}
    </p>
    <p class="notice" role="alert">
      {#if notice}<span class="glyph" aria-hidden="true">⚠</span>
        <span>{t(notice.key, notice.params)}</span>{/if}
    </p>

    <div class="trust">
      <span class="check">
        <input
          id="signin-trust"
          type="checkbox"
          bind:checked={trust}
          aria-describedby="signin-trust-hint"
        />
        <span class="box" aria-hidden="true">{trust ? "✓" : ""}</span>
      </span>
      <label for="signin-trust">{t("onboarding.signin.trust")}</label>
    </div>
    <p class="hint" id="signin-trust-hint">{t("onboarding.signin.trust_hint")}</p>

    <div class="alternatives">
      {#if mode !== "totp"}
        <button type="button" class="ui-interactive link-button" onclick={() => useMethod("totp")}>
          {t("onboarding.signin.use_totp")}
        </button>
      {/if}
      {#if mode !== "backup_code"}
        <button
          type="button"
          class="ui-interactive link-button"
          onclick={() => useMethod("backup_code")}
        >
          {t("onboarding.signin.use_backup")}
        </button>
      {/if}
      {#if methods.includes("sms") && mode !== "sms"}
        <button type="button" class="ui-interactive link-button" onclick={textMe}>
          {t(smsBusy ? "onboarding.signin.sms_sending" : "onboarding.signin.text_me")}
        </button>
      {/if}
      <button type="button" class="ui-interactive link-button" onclick={startOver}>
        {t("onboarding.signin.start_over")}
      </button>
    </div>
  {/if}
{:else}
  <h1 bind:this={heading} tabindex="-1">{t(lowTitle.key, lowTitle.params)}</h1>
  <p class="intro">{t("onboarding.signin.low_body")}</p>
  <div class="actions">
    <Button label="onboarding.signin.regenerate_now" onclick={() => goto("/account/security")} />
    <Button
      label="onboarding.signin.later"
      variant="secondary"
      onclick={() => goto("/account-setup")}
    />
  </div>
{/if}

<style>
  h1 {
    margin: var(--space-4) 0 var(--space-2);
    font-family: var(--type-web-display-family), system-ui, sans-serif;
    font-size: var(--type-web-display-size);
    font-weight: var(--type-web-display-weight);
    line-height: var(--type-web-display-line-height);
    letter-spacing: var(--type-web-display-letter-spacing);
  }

  h1:focus {
    outline: none;
  }

  .intro {
    margin: 0 0 var(--space-5);
    line-height: var(--type-hub-label-line-height);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .notice {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
    margin: var(--space-3) 0 0;
    color: var(--text-field-error-text);
    font-weight: var(--type-hub-title-weight);
  }

  .notice:empty {
    min-height: 0;
  }

  .actions,
  .alternatives {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .actions {
    margin-top: var(--space-3);
  }

  .alternatives {
    margin-top: var(--space-4);
  }

  .status,
  .countdown {
    margin: var(--space-3) 0 0;
    min-height: 1lh;
  }

  .aside {
    margin-top: var(--space-5);
  }

  .link {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
  }

  .link-button {
    --ui-bg: transparent;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    padding: 0 var(--space-2);
    border: none;
    border-radius: var(--radius-sm);
    color: var(--accent-text);
    font: inherit;
    font-weight: var(--type-hub-title-weight);
    text-decoration: underline;
    cursor: pointer;
  }

  .trust {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-4);
  }

  .trust label {
    font-weight: var(--type-hub-title-weight);
    cursor: pointer;
  }

  .hint {
    margin: var(--space-1) 0 0;
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
  }

  .check {
    position: relative;
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: var(--space-touch-min);
    height: var(--space-touch-min);
  }

  .check input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }

  .box {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    width: 1.5rem;
    height: 1.5rem;
    border: 2px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--text-field-background);
    color: var(--text-field-foreground);
    font-weight: var(--type-hub-title-weight);
    pointer-events: none;
  }

  .check:has(input:checked) .box {
    border-color: var(--accent);
  }

  .check:has(input:focus-visible) .box {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
