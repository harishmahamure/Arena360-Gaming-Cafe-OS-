<script lang="ts">
  import { Button, TextField, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { tick } from "svelte";
  import {
    declineStepUp,
    pickMethod,
    sendStepUpSms,
    stepup,
    submitStepUp,
    type StepMethod,
  } from "./commands.svelte";
  import { shell } from "./shell.svelte";

  let dialog: HTMLDialogElement | undefined = $state();
  let code = $state("");

  const HINTS: Record<StepMethod, MessageKey> = {
    totp: "owner.stepup.hint.totp",
    backup_code: "owner.stepup.hint.backup",
    sms: "owner.stepup.hint.sms",
  };

  // The dialog exists only while a step-up is asked for. It opens modal (focus is trapped inside)
  // and focus moves to the code field.
  $effect(() => {
    if (!dialog || dialog.open) return;
    code = "";
    dialog.showModal();
    void tick().then(() => dialog?.querySelector("input")?.focus());
  });

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    void submitStepUp(code.trim());
  };

  // Esc: the browser closes the dialog; the step-up is declined and nothing is sent.
  const cancel = (event: Event) => {
    event.preventDefault();
    declineStepUp();
  };

  const pause = $derived(Math.max(1, Math.ceil(stepup.pausedS / 60)));
  const other = (method: StepMethod) => {
    pickMethod(method);
    code = "";
    void tick().then(() => dialog?.querySelector("input")?.focus());
  };
</script>

{#if stepup.open}
  <dialog
    bind:this={dialog}
    class="stepup"
    aria-labelledby="stepup-title"
    oncancel={cancel}
    onclose={() => stepup.open && declineStepUp()}
    data-component="step-up-dialog"
  >
    <form class="body" onsubmit={submit}>
      <h2 id="stepup-title" class="type-hub-title">{t("owner.stepup.title")}</h2>
      <p class="hint">{t(HINTS[stepup.method])}</p>
      {#if stepup.smsSent && stepup.method === "sms"}
        <p class="note" role="status">{t("owner.stepup.sms_sent")}</p>
      {/if}
      <TextField
        label="owner.stepup.code"
        bind:value={code}
        name="code"
        autocomplete="one-time-code"
        inputmode={stepup.method === "backup_code" ? "text" : "numeric"}
        error={stepup.error === "wrong" ? "owner.stepup.wrong" : null}
      />
      {#if stepup.error === "paused"}
        <p class="error" role="alert">{t("owner.stepup.paused", { minutes: pause })}</p>
      {:else if stepup.error === "failed"}
        <p class="error" role="alert">{t("owner.stepup.failed")}</p>
      {/if}
      {#if stepup.smsFailed}<p class="error" role="alert">{t("owner.stepup.sms_failed")}</p>{/if}
      <div class="actions">
        <Button
          type="submit"
          label="owner.stepup.confirm"
          disabled={stepup.sending || !code.trim()}
          disabledReason={stepup.sending ? "owner.stepup.checking" : "owner.stepup.empty"}
        />
        <Button label="owner.cmd.cancel" variant="secondary" onclick={declineStepUp} />
      </div>
      <div class="other">
        {#if stepup.method !== "totp"}
          <button type="button" class="link ui-interactive" onclick={() => other("totp")}
            >{t("owner.stepup.use_totp")}</button
          >
        {/if}
        {#if stepup.method !== "backup_code"}
          <button type="button" class="link ui-interactive" onclick={() => other("backup_code")}
            >{t("owner.stepup.use_backup")}</button
          >
        {/if}
        {#if shell.smsFallback}
          <button type="button" class="link ui-interactive" onclick={sendStepUpSms}
            >{t("owner.stepup.use_sms")}</button
          >
        {/if}
      </div>
    </form>
  </dialog>
{/if}

<style>
  .stepup {
    box-sizing: border-box;
    width: min(28rem, calc(100vw - 2 * var(--space-4)));
    padding: var(--space-5);
    color: var(--text);
    background-color: var(--surface);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
  }
  .stepup::backdrop {
    background-color: var(--bg);
    opacity: 0.8;
  }
  .body {
    display: grid;
    gap: var(--space-3);
  }
  h2,
  p {
    margin: 0;
  }
  .hint,
  .note {
    color: var(--text-muted);
  }
  .error {
    color: var(--danger);
  }
  .actions,
  .other {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
  .link {
    min-height: var(--space-touch-min);
    padding: 0;
    color: var(--text);
    background: none;
    border: 0;
    text-decoration: underline;
    font: inherit;
    cursor: pointer;
  }
</style>
