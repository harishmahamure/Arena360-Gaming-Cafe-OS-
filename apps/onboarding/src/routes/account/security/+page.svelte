<script lang="ts">
  import { Button, CodeField, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { SessionOwner } from "@arena/ui/generated";
  import { goto } from "$app/navigation";
  import { onMount, tick } from "svelte";
  import { ackBackupCodes, currentSession, regenerateBackupCodes } from "../../../lib/api";
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

  async function load() {
    failed = false;
    const res = await currentSession();
    if (res.ok) {
      owner = res.data.owner;
    } else if (res.offline || res.status >= 500) {
      offline = res.offline;
      failed = true;
    } else {
      await goto("/", { replaceState: true });
    }
  }

  onMount(load);

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
{/if}

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
