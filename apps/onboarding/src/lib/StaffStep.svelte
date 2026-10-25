<script lang="ts">
  import { Button, TextField, TierLock, focusFirstInvalid, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { InviteRole, InviteStatus, StaffInviteView } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import {
    getStaff,
    getWizard,
    inviteStaff,
    newStaffCode,
    skipStaff,
    type ApiFailure,
  } from "./api";
  import { digitsOf, minSec, validPhone, validText } from "./validate";
  import { advance, setWizard, STEP_NAMES } from "./wizard.svelte";

  const ROLES: Record<InviteRole, MessageKey> = {
    manager: "onboarding.staff.role.manager",
    counter: "onboarding.staff.role.counter",
    kitchen: "onboarding.staff.role.kitchen",
  };
  const STATUSES: Record<InviteStatus, MessageKey> = {
    waiting: "onboarding.staff.status.waiting",
    applied: "onboarding.staff.status.applied",
    failed: "onboarding.staff.status.failed",
  };
  const POLL_MS = 5000;

  let invites: StaffInviteView[] = $state([]);
  let loaded = $state(false);
  let name = $state("");
  let phone = $state("");
  let role: InviteRole = $state("manager");
  let nameError: MessageKey | null = $state(null);
  let phoneError: MessageKey | null = $state(null);
  let formError: string | null = $state(null);
  let busy = $state(false);
  let shown: { name: string; code: string } | null = $state(null);
  let copied: MessageKey | null = $state(null);
  let valueEl: HTMLElement | undefined = $state();
  let form: HTMLFormElement | undefined = $state();

  const grouped = (code: string) => [code.slice(0, 4), code.slice(4)].join(" ");

  function failure(f: ApiFailure): string {
    if (f.error.code === "STEP_NOT_READY") return t("onboarding.staff.error.not_ready");
    if (f.error.code === "RATE_LIMITED") {
      return t("onboarding.staff.error.rate_limited", { time: minSec(f.retryAfterS ?? 60) });
    }
    return t(f.offline ? "onboarding.error.offline" : "onboarding.error.generic");
  }

  function reason(i: StaffInviteView): MessageKey {
    const code = i.error?.code;
    if (code === "LOGIN_TAKEN") return "onboarding.staff.reason.login_taken";
    if (code === "USER_LIMIT") return "onboarding.staff.reason.user_limit";
    return "onboarding.staff.reason.other";
  }

  async function refresh() {
    const res = await getStaff();
    if (!res.ok) return;
    const before = invites.filter((i) => i.status === "waiting").map((i) => i.id);
    invites = res.data.invites;
    loaded = true;
    const landed = invites.some((i) => i.status === "applied" && before.includes(i.id));
    if (!landed) return;
    const wizard = await getWizard();
    if (!wizard.ok) return;
    if (wizard.data.current === "staff") setWizard(wizard.data);
    else await advance(wizard.data);
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    nameError = validText(name) ? null : "onboarding.staff.error.name";
    const e164 = digitsOf(phone);
    phoneError = validPhone(e164) ? null : "onboarding.staff.error.phone";
    if (nameError || phoneError) {
      if (form) focusFirstInvalid(form);
      return;
    }
    busy = true;
    formError = null;
    const res = await inviteStaff({ name: name.trim(), phone: e164, role });
    busy = false;
    if (!res.ok) {
      formError = failure(res);
      return;
    }
    shown = { name: res.data.invite.name, code: res.data.code };
    copied = null;
    name = "";
    phone = "";
    role = "counter";
    await refresh();
  }

  async function reissue(i: StaffInviteView) {
    if (busy) return;
    busy = true;
    formError = null;
    const res = await newStaffCode(i.id);
    busy = false;
    if (!res.ok) {
      formError = failure(res);
      return;
    }
    shown = { name: res.data.invite.name, code: res.data.code };
    copied = null;
    await refresh();
  }

  async function skip() {
    if (busy) return;
    busy = true;
    const res = await skipStaff();
    busy = false;
    if (!res.ok) {
      formError = failure(res);
      return;
    }
    await advance(res.data);
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(shown?.code ?? "");
      copied = "ui.pairing_code.copied";
    } catch {
      const range = document.createRange();
      if (valueEl) range.selectNodeContents(valueEl);
      window.getSelection()?.removeAllRanges();
      window.getSelection()?.addRange(range);
      copied = "ui.pairing_code.copy_hint";
    }
  }

  onMount(() => {
    refresh().then(() => {
      if (invites.length === 0) role = "manager";
      else role = "counter";
    });
    const timer = setInterval(() => {
      if (invites.some((i) => i.status === "waiting")) refresh();
    }, POLL_MS);
    return () => clearInterval(timer);
  });
</script>

<h1 tabindex="-1">{t(STEP_NAMES.staff)}</h1>
<p class="intro">{t("onboarding.staff.intro")}</p>

<form bind:this={form} onsubmit={submit} novalidate data-testid="staff-form">
  <TextField label="onboarding.staff.name" bind:value={name} autocomplete="off" error={nameError} />
  <TextField
    label="onboarding.staff.phone"
    type="tel"
    inputmode="tel"
    bind:value={phone}
    autocomplete="off"
    error={phoneError}
  />
  <fieldset class="roles">
    <legend>{t("onboarding.staff.role")}</legend>
    {#each Object.entries(ROLES) as [r, label] (r)}
      <label class="role">
        <input type="radio" name="role" value={r} bind:group={role} />
        <span>{t(label)}</span>
      </label>
    {/each}
  </fieldset>
  {#if formError}
    <p class="form-error" role="alert">
      <span class="glyph" aria-hidden="true">⚠</span><span>{formError}</span>
    </p>
  {/if}
  <div class="actions">
    <Button
      type="submit"
      label="onboarding.staff.invite"
      disabled={busy}
      disabledReason="onboarding.staff.inviting"
    />
  </div>
</form>

{#if shown}
  <section class="notice" aria-labelledby="staff-code-title" data-testid="staff-code-notice">
    <h2 id="staff-code-title">{t("onboarding.setup_code.title")}</h2>
    <p class="value" bind:this={valueEl} data-testid="staff-code-text">
      {t("staff.setup_code.issued", { name: shown.name, code: grouped(shown.code) })}
    </p>
    <p class="once">{t("onboarding.setup_code.once")}</p>
    <div class="actions">
      <Button label="ui.pairing_code.copy" variant="secondary" onclick={copy} />
    </div>
    <p class="once" role="status">
      {#if copied}{t(copied)}{/if}
    </p>
  </section>
{/if}

<section aria-labelledby="staff-list-title">
  <h2 id="staff-list-title">{t("onboarding.staff.list")}</h2>
  {#if loaded && invites.length === 0}
    <p>{t("onboarding.staff.empty")}</p>
  {:else}
    <ul class="invites" data-testid="staff-list">
      {#each invites as i (i.id)}
        <li class="invite" data-testid="invite-{i.id}">
          <p class="who">
            <strong>{i.name}</strong> · {t(ROLES[i.role])}
          </p>
          <p class="login">{t("onboarding.staff.login", { phone: i.phone })}</p>
          <p class="state" data-status={i.status}>
            {#if i.status === "failed"}<span aria-hidden="true">⚠</span>{/if}
            {t(STATUSES[i.status])}
          </p>
          {#if i.status === "failed"}
            <p class="login">{t(reason(i))}</p>
            {#if i.error?.code === "USER_LIMIT"}<TierLock tier="Basic" />{/if}
          {:else}
            <Button
              label="onboarding.staff.new_code"
              variant="secondary"
              onclick={() => reissue(i)}
            />
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<div class="actions skip">
  <Button label="onboarding.staff.skip" variant="secondary" onclick={skip} />
  <p class="once">{t("onboarding.staff.skip_note")}</p>
</div>

<style>
  .intro {
    margin: 0 0 var(--space-5);
    line-height: var(--type-hub-label-line-height);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    max-width: 32rem;
    margin-bottom: var(--space-5);
  }

  .roles {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-4);
    margin: 0;
    padding: 0;
    border: 0;
  }

  .roles legend {
    padding: 0;
    margin-bottom: var(--space-2);
    font-weight: var(--type-hub-title-weight);
  }

  .role {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
  }

  .role input {
    width: 1.5rem;
    height: 1.5rem;
    margin: 0;
    accent-color: var(--accent);
  }

  .notice {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0 0 var(--space-5);
    padding: var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
  }

  h2 {
    margin: 0 0 var(--space-3);
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  .notice h2 {
    margin: 0;
  }

  p {
    margin: 0;
    line-height: var(--type-hub-label-line-height);
  }

  .value {
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }

  .once,
  .login {
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
  }

  .invites {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .invite {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
  }

  .state {
    font-weight: var(--type-hub-title-weight);
  }

  .form-error {
    display: flex;
    gap: var(--space-2);
    color: var(--text-field-error-text);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }

  .skip {
    margin-top: var(--space-5);
  }
</style>
