<script lang="ts">
  import { Button, CodeField, TextField, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { SignupChallenge } from "@arena/ui/generated";
  import { goto } from "$app/navigation";
  import { onDestroy, tick } from "svelte";
  import { resendCode, startSignup, verifyCode, type ApiFailure } from "../lib/api";
  import {
    COUNTRIES,
    FIELDS,
    fieldError,
    serverFieldError,
    isField,
    minSec,
    phoneOf,
    type FieldName,
    type SignupForm,
  } from "../lib/validate";

  type Notice = { key: MessageKey; params?: Record<string, string | number> };

  let step: "form" | "code" | "sign_in" = $state("form");
  let form: SignupForm = $state({
    name: "",
    country: "+91",
    number: "",
    email: "",
    cafe_name: "",
    city: "",
  });
  let errors: Partial<Record<FieldName, MessageKey>> = $state({});
  let formError: Notice | null = $state(null);
  let sending = $state(false);
  let formEl: HTMLFormElement | undefined = $state();

  let challenge: SignupChallenge | null = $state(null);
  let sentTo = $state("");
  let code = $state("");
  let codeError: Notice | null = $state(null);
  let codeStatus: Notice | null = $state(null);
  let checking = $state(false);
  let resending = $state(false);
  let codeField: { focus: () => void } | undefined = $state();
  /** The challenge can't be resent any more (`OTP_EXPIRED` from resend): only a new start helps. */
  let resendGone = $state(false);
  /** Bumped for every new challenge and on "Change details": answers for an older round are
   * dropped, so a late reply never revives a challenge the owner has left. */
  let round = 0;

  /** When "Resend code" becomes usable (ms since the epoch). */
  let resendAt = $state(0);
  let now = $state(Date.now());
  /** Announced once at the start of a countdown and once at 0, never on every tick. */
  let resendAnnouncement: Notice | null = $state(null);
  let timer: ReturnType<typeof setInterval> | null = null;

  const waitS = $derived(Math.max(0, Math.ceil((resendAt - now) / 1000)));
  const canResend = $derived(waitS === 0 && !resending && !checking);

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

  onDestroy(stopTimer);

  function validate(field: FieldName) {
    const e = fieldError(form, field);
    if (e) errors[field] = e;
    else delete errors[field];
  }

  /** On blur: check a field once something was typed in it (or it already shows an error). */
  function blurred(field: FieldName, event: FocusEvent) {
    // Moving to "Send code" checks everything anyway; checking here first would shift the
    // button under the pointer between press and release.
    const to = event.relatedTarget;
    if (to instanceof HTMLButtonElement && to.type === "submit") return;
    const value = field === "phone" ? form.number : form[field];
    if (value.trim() !== "" || errors[field]) validate(field);
  }

  function rateLimited(f: ApiFailure): Notice {
    return {
      key: "onboarding.error.rate_limited",
      params: { time: minSec(f.retryAfterS ?? 60) },
    };
  }

  function failure(f: ApiFailure): Notice {
    if (f.error.code === "RATE_LIMITED") return rateLimited(f);
    return { key: f.offline ? "onboarding.error.offline" : "onboarding.error.generic" };
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (sending) return;
    formError = null;
    for (const field of FIELDS) validate(field);
    if (Object.keys(errors).length > 0) {
      await tick();
      focusFirstInvalid(formEl);
      return;
    }
    sending = true;
    const res = await sendStart();
    sending = false;
    if (res.ok) return;
    const field = res.error.field;
    if (res.error.code === "VALIDATION_FAILED" && isField(field)) {
      errors[field] = fieldError(form, field) ?? serverFieldError(form, field);
      await tick();
      focusFirstInvalid(formEl);
      return;
    }
    formError = failure(res);
  }

  /** Starts a new challenge with the form as it is; on success the code step shows it. */
  async function sendStart() {
    const phone = phoneOf(form);
    const mine = ++round;
    const res = await startSignup({
      name: form.name.trim(),
      phone,
      email: form.email.trim(),
      cafe_name: form.cafe_name.trim(),
      city: form.city.trim(),
    });
    if (res.ok && mine === round) {
      challenge = res.data;
      sentTo = phone;
      code = "";
      codeError = null;
      codeStatus = null;
      resendGone = false;
      step = "code";
      countdown(res.data.resend_after_s);
    }
    return res;
  }

  /** "Send a new code" once the challenge can't be resent: a fresh start, same details. */
  async function sendNew() {
    if (sending) return;
    sending = true;
    codeError = null;
    const res = await sendStart();
    sending = false;
    if (!res.ok) codeError = failure(res);
    await tick();
    codeField?.focus();
  }

  /** Whether an answer for `id` in round `mine` still belongs to the challenge on screen. */
  const current = (id: string, mine: number) =>
    mine === round && challenge?.challenge_id === id && step === "code";

  async function verify(value: string) {
    if (!challenge || checking) return;
    const id = challenge.challenge_id;
    const mine = round;
    checking = true;
    codeError = null;
    codeStatus = { key: "onboarding.code.checking" };
    const res = await verifyCode(id, value);
    if (!current(id, mine)) return;
    checking = false;
    codeStatus = null;
    if (res.ok) {
      if (res.data.next === "account_setup") {
        stopTimer();
        await goto("/account-setup");
      } else {
        stopTimer();
        step = "sign_in";
      }
      return;
    }
    code = "";
    if (res.error.code === "OTP_INVALID") {
      codeError = { key: "onboarding.code.invalid" };
    } else if (res.error.code === "OTP_EXPIRED") {
      codeError = { key: "onboarding.code.expired" };
      countdown(0);
    } else {
      codeError = failure(res);
    }
    await tick();
    codeField?.focus();
  }

  async function resend() {
    if (!challenge || !canResend) return;
    const id = challenge.challenge_id;
    const mine = round;
    resending = true;
    codeError = null;
    const res = await resendCode(id);
    if (!current(id, mine)) return;
    resending = false;
    code = "";
    if (res.ok) {
      challenge = res.data;
      codeStatus = { key: "onboarding.code.resent" };
      countdown(res.data.resend_after_s);
    } else if (res.error.code === "RATE_LIMITED") {
      codeError = rateLimited(res);
      countdown(res.retryAfterS ?? 30);
    } else if (res.error.code === "OTP_EXPIRED") {
      codeError = { key: "onboarding.code.expired" };
      resendGone = true;
    } else {
      codeError = failure(res);
    }
    await tick();
    codeField?.focus();
  }

  function changeDetails() {
    stopTimer();
    round++;
    checking = false;
    resending = false;
    resendGone = false;
    challenge = null;
    code = "";
    codeError = null;
    codeStatus = null;
    step = "form";
  }
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("onboarding.start.title") })}</title>
</svelte:head>

{#if step === "form"}
  <h1>{t("onboarding.start.title")}</h1>
  <p class="intro">{t("onboarding.start.intro")}</p>
  <form class="form" bind:this={formEl} novalidate onsubmit={submit}>
    <div class="field" onfocusout={(e) => blurred("name", e)}>
      <TextField
        label="onboarding.field.name"
        name="name"
        autocomplete="name"
        required
        bind:value={form.name}
        error={errors.name ?? null}
      />
    </div>
    <div class="phone">
      <div class="country">
        <label class="label" for="signup-country">{t("onboarding.field.country")}</label>
        <select
          id="signup-country"
          class="ui-interactive select"
          name="country"
          autocomplete="tel-country-code"
          bind:value={form.country}
          onchange={() => errors.phone && validate("phone")}
        >
          {#each COUNTRIES as c (c.code)}
            <option value={c.code}>{t(c.label)}</option>
          {/each}
        </select>
      </div>
      <div class="field number" onfocusout={(e) => blurred("phone", e)}>
        <TextField
          label="onboarding.field.phone"
          name="phone"
          type="tel"
          inputmode="tel"
          autocomplete="tel-national"
          required
          bind:value={form.number}
          error={errors.phone ?? null}
        />
      </div>
    </div>
    <div class="field" onfocusout={(e) => blurred("email", e)}>
      <TextField
        label="onboarding.field.email"
        name="email"
        type="email"
        inputmode="email"
        autocomplete="email"
        required
        bind:value={form.email}
        error={errors.email ?? null}
      />
    </div>
    <div class="field" onfocusout={(e) => blurred("cafe_name", e)}>
      <TextField
        label="onboarding.field.cafe_name"
        name="cafe_name"
        autocomplete="organization"
        required
        bind:value={form.cafe_name}
        error={errors.cafe_name ?? null}
      />
    </div>
    <div class="field" onfocusout={(e) => blurred("city", e)}>
      <TextField
        label="onboarding.field.city"
        name="city"
        autocomplete="address-level2"
        required
        bind:value={form.city}
        error={errors.city ?? null}
      />
    </div>
    <p class="form-error" role="alert">
      {#if formError}<span class="glyph" aria-hidden="true">⚠</span>
        <span>{t(formError.key, formError.params)}</span>{/if}
    </p>
    <div class="actions">
      <Button
        label="onboarding.start.send"
        type="submit"
        disabled={sending}
        disabledReason="onboarding.start.sending"
      />
    </div>
  </form>
{:else if step === "code"}
  <h1>{t("onboarding.code.title")}</h1>
  <p class="intro">{t("onboarding.code.sent", { phone: sentTo })}</p>
  <div class="code">
    <CodeField
      bind:this={codeField}
      bind:value={code}
      label="onboarding.code.label"
      error={codeError?.key ?? null}
      errorParams={codeError?.params}
      busy={checking}
      oncomplete={verify}
    />
    <p class="status" aria-live="polite">
      {#if codeStatus}{t(codeStatus.key, codeStatus.params)}{/if}
    </p>
    <div class="actions">
      {#if resendGone}
        <Button
          label="onboarding.code.send_new"
          onclick={sendNew}
          disabled={sending}
          disabledReason="onboarding.start.sending"
        />
      {:else}
        <Button
          label="onboarding.code.resend"
          variant="secondary"
          onclick={resend}
          disabled={!canResend}
          disabledReason="onboarding.code.resend_wait"
        />
      {/if}
      <Button label="onboarding.code.change" variant="secondary" onclick={changeDetails} />
    </div>
    {#if waitS > 0 && !resendGone}
      <p class="countdown" data-testid="resend-countdown">
        {t("onboarding.code.resend_in", { time: minSec(waitS) })}
      </p>
    {/if}
    <p class="visually-hidden" aria-live="polite" data-testid="resend-announcement">
      {#if resendAnnouncement}{t(resendAnnouncement.key, resendAnnouncement.params)}{/if}
    </p>
  </div>
{:else}
  <h1>{t("onboarding.start.title")}</h1>
  <p class="intro" role="status">{t("onboarding.sign_in.message")}</p>
  <p><a class="link" href="/sign-in">{t("onboarding.sign_in.link")}</a></p>
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

  .intro {
    margin: 0 0 var(--space-5);
    line-height: var(--type-hub-label-line-height);
  }

  .form,
  .code {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .phone {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .country {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    flex: 0 1 12rem;
    min-width: 0;
  }

  .number {
    flex: 1 1 14rem;
    min-width: 0;
  }

  .label {
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
    line-height: var(--type-hub-label-line-height);
  }

  .select {
    --ui-bg: var(--text-field-background);
    min-height: var(--space-touch-min);
    width: 100%;
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
    font-size: var(--type-web-body-size);
  }

  .form-error {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
    margin: 0;
    color: var(--text-field-error-text);
    font-weight: var(--type-hub-title-weight);
  }

  .form-error:empty {
    min-height: 0;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .status,
  .countdown {
    margin: 0;
    min-height: 1lh;
  }

  .link {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
  }
</style>
