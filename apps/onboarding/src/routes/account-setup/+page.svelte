<script lang="ts">
  import { Button, CodeField, PasswordField, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { SessionOwner, TotpStarted } from "@arena/ui/generated";
  import { goto } from "$app/navigation";
  import { onMount, tick } from "svelte";
  import {
    ackBackupCodes,
    confirmTotp,
    currentSession,
    regenerateBackupCodes,
    setPassword,
    startTotp,
    type ApiFailure,
  } from "../../lib/api";
  import BackupCodesPanel from "../../lib/BackupCodesPanel.svelte";
  import TotpSetup from "../../lib/TotpSetup.svelte";

  type Step = "loading" | "retry" | "password" | "totp" | "codes" | "unacked" | "done";
  type Notice = { key: MessageKey; params?: Record<string, string | number> };

  let step = $state<Step>("loading");
  let heading: HTMLHeadingElement | undefined = $state();
  let offline = $state(false);

  let password = $state("");
  let passwordError: MessageKey | null = $state(null);
  let formError: Notice | null = $state(null);
  let saving = $state(false);
  let passwordField: { focus: () => void } | undefined = $state();

  let totp: TotpStarted | null = $state(null);
  let code = $state("");
  let codeError: MessageKey | null = $state(null);
  let codeStatus: MessageKey | null = $state(null);
  let checking = $state(false);
  let codeField: { focus: () => void } | undefined = $state();

  let backupCodes: string[] = $state([]);
  let regenerating = $state(false);

  async function beginRegenerate() {
    regenerating = true;
    code = "";
    codeError = null;
    await tick();
    codeField?.focus();
  }

  const progress = $derived(
    step === "password" ? 1 : step === "totp" ? 2 : step === "codes" ? 3 : null,
  );

  function failure(f: ApiFailure): Notice {
    return { key: f.offline ? "onboarding.error.offline" : "onboarding.error.generic" };
  }

  async function show(next: Step) {
    step = next;
    await tick();
    heading?.focus();
  }

  function stepFor(owner: SessionOwner): Step {
    if (!owner.has_password) return "password";
    if (owner.mfa_state === "pending") return "totp";
    return owner.backup_codes_acked ? "done" : "unacked";
  }

  async function enter(next: Step, moveFocus: boolean) {
    if (next === "totp") {
      totp = null;
      formError = null;
      const res = await startTotp();
      if (res.ok) totp = res.data;
      else if (res.error.code === "PASSWORD_REQUIRED") next = "password";
      else if (res.error.code === "ALREADY_SET") return reload(moveFocus);
      else formError = failure(res);
    }
    code = "";
    codeError = null;
    regenerating = false;
    if (moveFocus) await show(next);
    else step = next;
  }

  async function reload(moveFocus: boolean) {
    const res = await currentSession();
    if (res.ok) {
      await enter(stepFor(res.data.owner), moveFocus);
    } else if (res.offline || res.status >= 500) {
      offline = res.offline;
      step = "retry";
    } else {
      await goto("/", { replaceState: true });
    }
  }

  function retry() {
    step = "loading";
    return reload(false);
  }

  onMount(() => reload(false));

  async function submitPassword(event: SubmitEvent) {
    event.preventDefault();
    if (saving) return;
    formError = null;
    const n = [...password].length;
    passwordError = n < 10 ? "onboarding.password.too_short" : null;
    if (passwordError) {
      passwordField?.focus();
      return;
    }
    saving = true;
    const res = await setPassword(password);
    saving = false;
    if (res.ok || res.error.code === "ALREADY_SET") {
      password = "";
      await enter("totp", true);
      return;
    }
    const byCode: Partial<Record<string, MessageKey>> = {
      PASSWORD_TOO_SHORT: "onboarding.password.too_short",
      PASSWORD_COMMON: "onboarding.password.common",
      VALIDATION_FAILED: "onboarding.password.too_long",
    };
    const key = byCode[res.error.code];
    if (key) {
      passwordError = key;
      passwordField?.focus();
    } else {
      formError = failure(res);
    }
  }

  async function checkCode(value: string, regenerate: boolean) {
    if (checking) return;
    checking = true;
    codeError = null;
    codeStatus = "onboarding.totp.checking";
    const res = regenerate ? await regenerateBackupCodes(value) : await confirmTotp(value);
    checking = false;
    codeStatus = null;
    if (res.ok) {
      backupCodes = res.data.backup_codes;
      await show("codes");
      return;
    }
    code = "";
    if (res.error.code === "TOTP_INVALID") {
      codeError = regenerate ? "onboarding.regenerate.invalid" : "onboarding.totp.invalid";
    } else if (res.error.code === "ALREADY_SET" || res.error.code === "FORBIDDEN") {
      await reload(true);
      return;
    } else {
      codeError = failure(res).key;
    }
    await tick();
    codeField?.focus();
  }

  async function saveCodes(): Promise<boolean> {
    const res = await ackBackupCodes();
    if (!res.ok) return false;
    backupCodes = [];
    await show("done");
    return true;
  }
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("onboarding.account_setup.title") })}</title>
</svelte:head>

{#if progress}
  <p class="progress">{t("onboarding.account_setup.progress", { n: progress })}</p>
{/if}

{#if step === "retry"}
  <h1>{t("onboarding.account_setup.title")}</h1>
  <p role="alert">{t(offline ? "onboarding.error.offline" : "onboarding.error.generic")}</p>
  <Button label="onboarding.account_setup.retry" onclick={retry} />
{:else if step === "loading"}
  <h1>{t("onboarding.account_setup.title")}</h1>
  <p role="status">{t("onboarding.account_setup.checking")}</p>
{:else if step === "password"}
  <h1 bind:this={heading} tabindex="-1">{t("onboarding.password.title")}</h1>
  <p class="intro">{t("onboarding.password.intro")}</p>
  <form class="form" novalidate onsubmit={submitPassword}>
    <PasswordField
      bind:this={passwordField}
      bind:value={password}
      label="onboarding.password.label"
      name="password"
      autocomplete="new-password"
      required
      hint="onboarding.password.hint"
      error={passwordError}
    />
    <p class="form-error" role="alert">
      {#if formError}<span class="glyph" aria-hidden="true">⚠</span>
        <span>{t(formError.key, formError.params)}</span>{/if}
    </p>
    <div class="actions">
      <Button
        label="onboarding.password.submit"
        type="submit"
        disabled={saving}
        disabledReason="onboarding.password.saving"
      />
    </div>
  </form>
{:else if step === "totp"}
  <h1 bind:this={heading} tabindex="-1">{t("onboarding.totp.title")}</h1>
  <p class="intro">{t("onboarding.totp.intro")}</p>
  {#if totp}
    <div class="stack">
      <TotpSetup {totp} />
      <CodeField
        bind:this={codeField}
        bind:value={code}
        label="onboarding.totp.code_label"
        error={codeError}
        busy={checking}
        autofocus={false}
        oncomplete={(v) => checkCode(v, false)}
      />
      <p class="status" aria-live="polite">
        {#if codeStatus}{t(codeStatus)}{/if}
      </p>
    </div>
  {:else if formError}
    <p class="form-error" role="alert">
      <span class="glyph" aria-hidden="true">⚠</span><span>{t(formError.key)}</span>
    </p>
    <div class="actions">
      <Button label="onboarding.account_setup.retry" onclick={() => enter("totp", false)} />
    </div>
  {:else}
    <p role="status">{t("onboarding.totp.loading")}</p>
  {/if}
{:else if step === "codes"}
  <h1 bind:this={heading} tabindex="-1">{t("onboarding.codes.title")}</h1>
  <p class="intro">{t("onboarding.codes.intro")}</p>
  <BackupCodesPanel codes={backupCodes} onsave={saveCodes} />
{:else if step === "unacked"}
  <h1 bind:this={heading} tabindex="-1">{t("onboarding.unacked.title")}</h1>
  <p class="intro">{t("onboarding.unacked.intro")}</p>
  {#if regenerating}
    <div class="stack">
      <CodeField
        bind:this={codeField}
        bind:value={code}
        label="onboarding.totp.code_label"
        error={codeError}
        busy={checking}
        autofocus={false}
        oncomplete={(v) => checkCode(v, true)}
      />
      <p class="status" aria-live="polite">
        {#if codeStatus}{t(codeStatus)}{/if}
      </p>
    </div>
  {:else}
    <div class="actions">
      <Button label="onboarding.regenerate.button" onclick={beginRegenerate} />
    </div>
  {/if}
{:else}
  <h1 bind:this={heading} tabindex="-1">{t("onboarding.done.title")}</h1>
  <p class="intro">{t("onboarding.done.body")}</p>
  <p><a class="link" href="/account/security">{t("onboarding.done.security")}</a></p>
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

  .progress {
    margin: var(--space-4) 0 0;
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
  }

  .intro {
    margin: 0 0 var(--space-5);
    line-height: var(--type-hub-label-line-height);
  }

  .form,
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
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

  .status {
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
