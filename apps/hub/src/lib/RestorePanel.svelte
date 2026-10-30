<script lang="ts">
  import { Button, InlinePin, t } from "@arena/ui";
  import type { RestoreCheck } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY } from "./auth";
  import { exportedAt } from "./backup";
  import { branchLocale } from "./locale.svelte";
  import {
    checkRestore,
    confirmRestore,
    type CheckOutcome,
    type RestoreFailure,
    type RestoreOutcome,
  } from "./restore";

  // Settings → System → "Restore from file" (Story 9.7). Only the Owner restores: the Owner checks
  // with their session and confirms with their PIN; a Manager or Counter is asked for the Owner's PIN
  // in place, at the check and again at the confirmation. The confirmation shows what will be set
  // aside and wants the branch code typed. The restore itself runs when the Hub starts again.
  let {
    canRestore,
  }: {
    /** The signed-in member holds `ops.restore` (the Owner). */
    canRestore: boolean;
  } = $props();

  type Step = "idle" | "check_pin" | "checked" | "confirm_pin" | "restarting";
  type PinResult = { ok: true } | { ok: false; message: MessageKey; retryAfterS?: number };

  let step: Step = $state("idle");
  let path = $state("");
  let code = $state("");
  let busy = $state(false);
  let problem: MessageKey | null = $state(null);
  let check: RestoreCheck | null = $state(null);

  const tz = $derived(branchLocale.current?.tz);
  const canCheck = $derived(path.trim().length > 0 && !busy && step === "idle");
  const canConfirm = $derived(code.trim().length > 0);

  function reset() {
    step = "idle";
    check = null;
    code = "";
    problem = null;
  }

  /** What a failed answer means for an open PIN entry, or `null` when the PIN itself was fine. */
  function pinFeedback(outcome: RestoreFailure): PinResult | null {
    switch (outcome.kind) {
      case "pin_rejected":
        return { ok: false, message: "ui.pin.rejected" };
      case "locked":
        return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS };
      case "signed_out":
        void invalidate(SESSION_DEPENDENCY);
        return { ok: false, message: "ui.pin.unreachable" };
      case "failed":
        return { ok: false, message: "ui.pin.unreachable" };
      case "problem":
        return outcome.message === "hub.restore.problem.denied"
          ? { ok: false, message: outcome.message }
          : null;
    }
  }

  function accepted(outcome: CheckOutcome) {
    if (outcome.kind === "ok") {
      check = outcome.check;
      code = "";
      problem = null;
      step = "checked";
    } else if (outcome.kind === "problem") {
      problem = outcome.message;
      step = "idle";
    }
  }

  /** Check with the Owner's session. */
  async function checkAsOwner() {
    if (!canCheck) return;
    busy = true;
    problem = null;
    let outcome: CheckOutcome;
    try {
      outcome = await checkRestore(path.trim());
    } finally {
      busy = false;
    }
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "failed") problem = "hub.restore.problem.failed";
    else accepted(outcome);
  }

  function tapCheck() {
    if (!canCheck) return;
    problem = null;
    if (canRestore) void checkAsOwner();
    else step = "check_pin";
  }

  async function checkWithPin(pin: string): Promise<PinResult> {
    const outcome = await checkRestore(path.trim(), pin);
    if (outcome.kind !== "ok") {
      const feedback = pinFeedback(outcome);
      if (feedback) return feedback;
    }
    accepted(outcome);
    return { ok: true };
  }

  async function restoreWithPin(pin: string): Promise<PinResult> {
    const outcome: RestoreOutcome = await confirmRestore(path.trim(), pin, code.trim());
    if (outcome.kind === "ok") {
      step = "restarting";
      return { ok: true };
    }
    const feedback = pinFeedback(outcome);
    if (feedback) return feedback;
    if (outcome.kind === "problem") {
      problem = outcome.message;
      // A wrong code can be typed again; anything else about the file starts over.
      step = outcome.message === "hub.restore.problem.branch_code" ? "checked" : "idle";
      if (step === "idle") check = null;
    }
    return { ok: true };
  }
</script>

<section class="restore" aria-labelledby="settings-restore-title" data-testid="settings-restore">
  <h3 class="type-hub-title" id="settings-restore-title">{t("hub.restore.title")}</h3>
  <p class="intro type-hub-body">{t("hub.restore.intro")}</p>

  {#if step !== "restarting"}
    <label class="field type-hub-label">
      <span>{t("hub.restore.path_label")}</span>
      <input
        class="ui-interactive"
        type="text"
        bind:value={path}
        disabled={step !== "idle" || busy}
        autocomplete="off"
        spellcheck="false"
        aria-describedby="restore-path-hint"
        data-testid="restore-path"
      />
      <span class="hint type-hub-label" id="restore-path-hint">{t("hub.restore.path_hint")}</span>
    </label>

    {#if step === "idle"}
      <div class="row">
        <Button
          label={busy ? "hub.restore.checking" : "hub.restore.check"}
          disabled={!canCheck}
          disabledReason={busy ? "hub.restore.checking" : "hub.restore.problem.not_set"}
          onclick={tapCheck}
        />
      </div>
    {/if}
  {/if}

  {#if step === "check_pin"}
    <div class="approval" data-testid="restore-approval">
      <InlinePin message="hub.restore.owner_pin_prompt" onsubmit={checkWithPin} oncancel={reset} />
    </div>
  {/if}

  {#if check && (step === "checked" || step === "confirm_pin")}
    <div class="summary" data-testid="restore-summary">
      <h4 class="type-hub-label">{t("hub.restore.summary_title")}</h4>
      <ul class="type-hub-body">
        <li>
          {t("hub.restore.summary_branch", { name: check.branch_name, code: check.branch_code })}
        </li>
        <li>{t("hub.restore.summary_exported", { time: exportedAt(check.exported_at, tz) })}</li>
        <li>
          {t("hub.restore.summary_epoch", {
            epoch: check.epoch || "-",
            seq: String(check.last_seq),
          })}
        </li>
        <li>{t("hub.restore.summary_schema", { version: String(check.schema_version) })}</li>
        <li data-testid="restore-set-aside">
          {t("hub.restore.summary_set_aside", { n: String(check.rows_set_aside) })}
        </li>
      </ul>
      <p class="warning type-hub-body" role="note" data-testid="restore-warning">
        <span aria-hidden="true">⚠</span>
        <span>{check.warning}</span>
      </p>
    </div>

    {#if step === "checked"}
      <label class="field type-hub-label">
        <span>{t("hub.restore.code_label", { code: check.branch_code })}</span>
        <input
          class="ui-interactive"
          type="text"
          bind:value={code}
          autocomplete="off"
          spellcheck="false"
          data-testid="restore-code"
        />
      </label>
      <div class="row">
        <Button
          label="hub.restore.restore"
          disabled={!canConfirm}
          disabledReason="hub.restore.problem.branch_code"
          onclick={() => {
            problem = null;
            step = "confirm_pin";
          }}
        />
        <Button label="hub.restore.start_again" variant="secondary" onclick={reset} />
      </div>
    {:else}
      <div class="approval" data-testid="restore-approval">
        <InlinePin
          message="hub.restore.confirm_pin_prompt"
          onsubmit={restoreWithPin}
          oncancel={() => (step = "checked")}
        />
      </div>
    {/if}
  {/if}

  <div class="result" role="status" aria-live="polite" data-testid="restore-status">
    {#if problem}
      <p class="type-hub-label warn" data-testid="restore-problem">
        <span aria-hidden="true">⚠</span>
        <span>{t(problem)}</span>
      </p>
    {:else if step === "restarting"}
      <p class="type-hub-body" data-testid="restore-restarting">{t("hub.restore.restarting")}</p>
    {/if}
  </div>
</section>

<style>
  .restore {
    display: grid;
    gap: var(--space-3);
    max-width: 40rem;
    padding-top: var(--space-4);
    border-top: 1px solid var(--border-strong);
  }

  h3,
  h4,
  .intro,
  .summary ul,
  .result p,
  .warning {
    margin: 0;
  }

  .intro,
  .hint {
    color: var(--text-muted);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .summary,
  .approval {
    display: grid;
    gap: var(--space-2);
  }

  .summary ul {
    padding-inline-start: var(--space-5);
    overflow-wrap: anywhere;
  }

  .warning,
  .warn {
    display: flex;
    gap: var(--space-2);
    color: var(--text-field-error-text);
  }

  .result {
    min-height: 1lh;
  }
</style>
