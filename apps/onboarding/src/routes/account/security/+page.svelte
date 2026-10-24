<script lang="ts">
  import { Button, CodeField, formatDateTime, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { SessionOwner, TrustedDevice } from "@arena/ui/generated";
  import { goto } from "$app/navigation";
  import { onMount, tick } from "svelte";
  import {
    ackBackupCodes,
    currentSession,
    listDevices,
    regenerateBackupCodes,
    removeDevice,
    setSmsFallback,
    signOutEverywhere,
  } from "../../../lib/api";
  import BackupCodesPanel from "../../../lib/BackupCodesPanel.svelte";

  // Account → Security, gated by `GET /v1/session`. Enrolled owners can regenerate their backup
  // codes: a dialog asks for a current authenticator code, then shows the new codes once, as in
  // account setup.

  let owner: SessionOwner | null = $state(null);
  let dialog: HTMLDialogElement | undefined = $state();
  let opener: HTMLElement | null = null;
  let code = $state("");
  let codeError: MessageKey | null = $state(null);
  let checking = $state(false);
  let codes: string[] = $state([]);
  let done = $state(false);
  let codeField: { focus: () => void } | undefined = $state();
  let failed = $state(false);
  let offline = $state(false);

  let devices: TrustedDevice[] | null = $state(null);
  let devicesError: MessageKey | null = $state(null);
  let devicesStatus: MessageKey | null = $state(null);
  let smsBusy = $state(false);
  let smsStatus: MessageKey | null = $state(null);
  let signoutDialog: HTMLDialogElement | undefined = $state();
  let signoutOpener: HTMLElement | null = null;
  let signingOut = $state(false);
  let signoutError: MessageKey | null = $state(null);

  async function load() {
    failed = false;
    const res = await currentSession();
    if (res.ok) {
      owner = res.data.owner;
      if (owner.mfa_state === "enrolled") await loadDevices();
    } else if (res.offline || res.status >= 500) {
      offline = res.offline;
      failed = true;
    } else {
      await goto("/sign-in", { replaceState: true });
    }
  }

  onMount(load);

  async function loadDevices() {
    const res = await listDevices();
    if (res.ok) {
      devices = res.data;
      devicesError = null;
    } else {
      devicesError = res.offline ? "onboarding.error.offline" : "onboarding.error.generic";
    }
  }

  async function toggleSms() {
    if (!owner || smsBusy) return;
    const next = !owner.sms_fallback;
    smsBusy = true;
    smsStatus = null;
    const res = await setSmsFallback(next);
    smsBusy = false;
    if (res.ok) {
      owner = { ...owner, sms_fallback: next };
      smsStatus = next ? "onboarding.security.sms_on" : "onboarding.security.sms_off";
    } else {
      smsStatus = res.offline ? "onboarding.error.offline" : "onboarding.error.generic";
    }
  }

  let removing: string | null = $state(null);

  async function remove(id: string) {
    if (removing) return;
    removing = id;
    devicesStatus = null;
    const current = (devices ?? []).some((d) => d.id === id && d.current);
    const res = await removeDevice(id);
    if (!res.ok && res.status !== 404) {
      removing = null;
      devicesStatus = res.offline ? "onboarding.error.offline" : "onboarding.error.generic";
      return;
    }
    if (current) {
      await goto("/sign-in", { replaceState: true });
      return;
    }
    const session = await currentSession();
    if (!session.ok) {
      await goto("/sign-in", { replaceState: true });
      return;
    }
    owner = session.data.owner;
    await loadDevices();
    removing = null;
    devicesStatus = "onboarding.security.removed";
    await tick();
    document.getElementById("devices-heading")?.focus();
  }

  function openSignout(event: MouseEvent) {
    signoutOpener = event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
    signoutError = null;
    signoutDialog?.showModal();
  }

  function closeSignout() {
    signoutDialog?.close();
  }

  async function confirmSignout() {
    if (signingOut) return;
    signingOut = true;
    signoutError = null;
    const res = await signOutEverywhere();
    signingOut = false;
    if (res.ok || res.status === 401) {
      signoutDialog?.close();
      await goto("/sign-in", { replaceState: true });
      return;
    }
    signoutError = res.offline ? "onboarding.error.offline" : "onboarding.error.generic";
  }

  async function open(event: MouseEvent) {
    opener = event.currentTarget instanceof HTMLElement ? event.currentTarget : null;
    code = "";
    codeError = null;
    codes = [];
    done = false;
    dialog?.showModal();
    await tick();
    codeField?.focus();
  }

  const unsaved = () => codes.length > 0 && !done;

  function close() {
    if (!unsaved()) dialog?.close();
  }

  function closed() {
    if (unsaved()) {
      dialog?.showModal();
      return;
    }
    codes = [];
    code = "";
    opener?.focus();
  }

  function cancel(event: Event) {
    if (unsaved()) event.preventDefault();
  }

  async function check(value: string) {
    if (checking) return;
    checking = true;
    codeError = null;
    const res = await regenerateBackupCodes(value);
    checking = false;
    if (res.ok) {
      codes = res.data.backup_codes;
      return;
    }
    code = "";
    codeError =
      res.error.code === "TOTP_INVALID"
        ? "onboarding.regenerate.invalid"
        : res.offline
          ? "onboarding.error.offline"
          : "onboarding.error.generic";
    await tick();
    codeField?.focus();
  }

  async function saved(): Promise<boolean> {
    const res = await ackBackupCodes();
    if (!res.ok) return false;
    done = true;
    if (owner) owner = { ...owner, backup_codes_acked: true };
    close();
    return true;
  }
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("onboarding.security.title") })}</title>
</svelte:head>

<h1>{t("onboarding.security.title")}</h1>
{#if failed}
  <p role="alert">{t(offline ? "onboarding.error.offline" : "onboarding.error.generic")}</p>
  <Button label="onboarding.account_setup.retry" onclick={load} />
{:else if !owner}
  <p role="status">{t("onboarding.account_setup.checking")}</p>
{:else if owner.mfa_state !== "enrolled"}
  <p class="intro">{t("onboarding.security.not_enrolled")}</p>
  <p><a class="link" href="/account-setup">{t("onboarding.security.finish_setup")}</a></p>
{:else}
  <section class="section" aria-labelledby="codes-heading">
    <h2 id="codes-heading">{t("onboarding.security.codes_title")}</h2>
    <p class="intro">{t("onboarding.security.codes_body")}</p>
    <div class="actions">
      <Button label="onboarding.regenerate.button" variant="secondary" onclick={open} />
    </div>
    <p class="status" role="status">
      {#if done}{t("onboarding.security.done")}{/if}
    </p>
  </section>

  <section class="section" aria-labelledby="sms-heading">
    <h2 id="sms-heading">{t("onboarding.security.sms_title")}</h2>
    <p class="intro" id="sms-body">{t("onboarding.security.sms_body")}</p>
    <div class="switch-row">
      <button
        type="button"
        class="ui-interactive switch"
        role="switch"
        aria-checked={owner.sms_fallback}
        aria-labelledby="sms-label"
        aria-describedby="sms-body"
        aria-busy={smsBusy}
        onclick={toggleSms}
      >
        <span class="track" aria-hidden="true"><span class="thumb"></span></span>
      </button>
      <span class="switch-label" id="sms-label">{t("onboarding.security.sms_label")}</span>
    </div>
    <p class="status" role="status">
      {#if smsStatus}{t(smsStatus)}{/if}
    </p>
  </section>

  <section class="section" aria-labelledby="devices-heading">
    <h2 id="devices-heading" tabindex="-1">{t("onboarding.security.devices_title")}</h2>
    <p class="intro">{t("onboarding.security.devices_body")}</p>
    {#if devicesError}
      <p role="alert">{t(devicesError)}</p>
    {:else if devices === null}
      <p role="status">{t("onboarding.account_setup.checking")}</p>
    {:else if devices.length === 0}
      <p>{t("onboarding.security.devices_empty")}</p>
    {:else}
      <ul class="devices">
        {#each devices as d (d.id)}
          <li class="device">
            <div class="device-text">
              <span class="device-label">{d.label}</span>
              {#if d.current}
                <span class="tag">{t("onboarding.security.this_device")}</span>
              {/if}
              <span class="muted">
                {t("onboarding.security.last_seen", { time: formatDateTime(d.last_seen_at, null) })}
              </span>
            </div>
            <Button
              label="onboarding.security.remove"
              variant="secondary"
              onclick={() => remove(d.id)}
              disabled={removing !== null}
              disabledReason="onboarding.security.removing"
            />
          </li>
        {/each}
      </ul>
    {/if}
    <p class="status" role="status">
      {#if devicesStatus}{t(devicesStatus)}{/if}
    </p>
  </section>

  <section class="section" aria-labelledby="signout-heading">
    <h2 id="signout-heading">{t("onboarding.security.signout_title")}</h2>
    <p class="intro">{t("onboarding.security.signout_body")}</p>
    <div class="actions">
      <Button label="onboarding.security.signout_button" variant="warning" onclick={openSignout} />
    </div>
  </section>
{/if}

<dialog
  bind:this={signoutDialog}
  class="dialog"
  aria-labelledby="signout-dialog-heading"
  aria-describedby="signout-dialog-body"
  onclose={() => signoutOpener?.focus()}
>
  <h2 id="signout-dialog-heading">{t("onboarding.security.signout_confirm_title")}</h2>
  <p class="intro" id="signout-dialog-body">{t("onboarding.security.signout_confirm_body")}</p>
  <p class="error" role="alert">
    {#if signoutError}{t(signoutError)}{/if}
  </p>
  <div class="actions">
    <Button label="onboarding.security.cancel" variant="secondary" onclick={closeSignout} />
    <Button
      label="onboarding.security.signout_button"
      variant="warning"
      onclick={confirmSignout}
      disabled={signingOut}
      disabledReason="onboarding.signin.signing_in"
    />
  </div>
</dialog>

<dialog
  bind:this={dialog}
  class="dialog"
  aria-labelledby="regenerate-heading"
  onclose={closed}
  oncancel={cancel}
>
  <h2 id="regenerate-heading">
    {t(codes.length > 0 ? "onboarding.codes.title" : "onboarding.security.dialog_title")}
  </h2>
  {#if codes.length > 0}
    <p class="intro">{t("onboarding.codes.intro")}</p>
    <BackupCodesPanel {codes} onsave={saved} />
  {:else}
    <p class="intro">{t("onboarding.security.dialog_intro")}</p>
    <CodeField
      bind:this={codeField}
      bind:value={code}
      label="onboarding.totp.code_label"
      error={codeError}
      busy={checking}
      autofocus={false}
      oncomplete={check}
    />
    <div class="actions">
      <Button label="onboarding.security.cancel" variant="secondary" onclick={close} />
    </div>
  {/if}
</dialog>

<style>
  h1 {
    margin: var(--space-4) 0 var(--space-2);
    font-family: var(--type-web-display-family), system-ui, sans-serif;
    font-size: var(--type-web-display-size);
    font-weight: var(--type-web-display-weight);
    line-height: var(--type-web-display-line-height);
    letter-spacing: var(--type-web-display-letter-spacing);
  }

  h2 {
    margin: 0 0 var(--space-2);
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin-top: var(--space-5);
  }

  .intro {
    margin: 0 0 var(--space-3);
    line-height: var(--type-hub-label-line-height);
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

  .switch-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .switch {
    --ui-bg: transparent;
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    min-width: var(--space-touch-min);
    min-height: var(--space-touch-min);
    padding: 0;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .track {
    display: inline-flex;
    align-items: center;
    box-sizing: border-box;
    width: 2.75rem;
    height: 1.5rem;
    padding: 2px;
    border: 2px solid var(--border-strong);
    border-radius: 999px;
    background: var(--text-field-background);
  }

  .thumb {
    width: 1rem;
    height: 1rem;
    border-radius: 50%;
    background: var(--text-muted);
    transition: transform 120ms ease;
  }

  .switch[aria-checked="true"] .track {
    border-color: var(--accent);
    background: var(--accent);
  }

  .switch[aria-checked="true"] .thumb {
    background: var(--surface);
    transform: translateX(1.25rem);
  }

  .switch-label {
    font-weight: var(--type-hub-title-weight);
  }

  .devices {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .device {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--line);
  }

  .device-text {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }

  .device-label {
    font-weight: var(--type-hub-title-weight);
    overflow-wrap: anywhere;
  }

  .tag {
    align-self: flex-start;
    padding: 0 var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    font-size: var(--type-hub-label-size);
  }

  .muted {
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
  }

  .error {
    margin: 0;
    min-height: 1lh;
    color: var(--text-field-error-text);
    font-weight: var(--type-hub-title-weight);
  }

  h2:focus {
    outline: none;
  }

  .link {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
  }

  .dialog {
    box-sizing: border-box;
    width: min(36rem, calc(100vw - 2 * var(--space-4)));
    max-height: calc(100dvh - 2 * var(--space-4));
    padding: var(--space-5);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background: var(--surface);
    color: var(--text);
  }

  .dialog[open] {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
</style>
