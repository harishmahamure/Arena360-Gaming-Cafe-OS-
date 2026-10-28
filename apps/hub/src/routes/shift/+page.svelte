<script lang="ts">
  import { Button, TextField, focusFirstInvalid, formatDateTime, formatMoney, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onDestroy, onMount, tick } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, newIdempotencyKey, submitIntent } from "../../lib/auth";
  import { branchLocale } from "../../lib/locale.svelte";
  import { floatText, parseFloat } from "../../lib/shift";
  import { shiftState } from "../../lib/shift.svelte";
  import { SNAPSHOT_DEPENDENCY } from "../../lib/snapshot";
  import { page } from "$app/state";
  import CashMovePanel from "../../lib/CashMovePanel.svelte";
  import { sessionStaff } from "../../lib/operating";

  // The running count of cash events follows the Hub; the drawer total is never shown.
  const REFRESH_MS = 5000;

  const locale = $derived(branchLocale.current);
  const shift = $derived(shiftState.current);
  const open = $derived(shift?.open ?? null);
  const staff = $derived(sessionStaff(page.data));

  let opening = $state(false);
  let text = $state("");
  let error: MessageKey | null = $state(null);
  let refusal: string | null = $state(null);
  let busy = $state(false);
  let opened = $state(false);
  let root: HTMLElement | undefined = $state();
  let attempt: { key: string; float: number } | null = null;
  let timer: ReturnType<typeof setInterval> | undefined;

  onMount(() => {
    void invalidate(SNAPSHOT_DEPENDENCY);
    timer = setInterval(() => void invalidate(SNAPSHOT_DEPENDENCY), REFRESH_MS);
  });
  onDestroy(() => clearInterval(timer));

  async function begin() {
    if (!locale || !shift) return;
    text = floatText(shift.carried_minor, locale.exponent);
    error = null;
    refusal = null;
    opened = false;
    opening = true;
    await tick();
    root?.querySelector<HTMLInputElement>("input")?.select();
  }

  async function confirm(event: SubmitEvent) {
    event.preventDefault();
    if (busy || !locale) return;
    const float = parseFloat(text, locale.exponent);
    if (float === null) {
      error = "hub.shift.err.float";
      await tick();
      return focusFirstInvalid(root);
    }
    error = null;
    refusal = null;
    busy = true;
    // A retry of the same float reuses its key, so the Hub opens one shift.
    const key = attempt && attempt.float === float ? attempt.key : newIdempotencyKey();
    attempt = { key, float };
    const outcome = await submitIntent({ kind: "shift.open", float_minor: float }, key).finally(
      () => (busy = false),
    );
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      opening = false;
      opened = true;
      attempt = null;
      return void (await invalidate(SNAPSHOT_DEPENDENCY));
    }
    if (outcome.kind === "error") {
      const err = outcome.error;
      if (err.code === "SHIFT_ALREADY_OPEN") {
        opening = false;
        refusal = err.message;
        return void (await invalidate(SNAPSHOT_DEPENDENCY));
      }
      if (err.code === "VALIDATION_FAILED" && err.field === "float_minor") {
        error = "hub.shift.err.float";
        await tick();
        return focusFirstInvalid(root);
      }
      refusal =
        err.code === "PERMISSION_DENIED" ? t("hub.refusal.permission") : t("hub.shift.failed");
      return;
    }
    refusal = t("hub.shift.failed");
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.nav.shift") })}</title>
</svelte:head>

<section class="shift" aria-labelledby="shift-title" bind:this={root}>
  <h1 class="type-hub-numeric" id="shift-title">{t("hub.nav.shift")}</h1>

  {#if !shift || !locale}
    <p class="type-hub-body" role="alert" data-testid="shift-load-failed">
      {t("hub.shift.load_failed")}
    </p>
    <div class="actions">
      <Button
        label="common.retry"
        variant="secondary"
        onclick={() => void invalidate(SNAPSHOT_DEPENDENCY)}
      />
    </div>
  {:else if open}
    <h2 class="type-hub-title">{t("hub.shift.current")}</h2>
    <dl class="facts type-hub-body" data-testid="shift-open">
      <div>
        <dt class="type-hub-label">{t("hub.shift.opened_by")}</dt>
        <dd data-testid="shift-opened-by">{open.staff_name}</dd>
      </div>
      <div>
        <dt class="type-hub-label">{t("hub.shift.opened_at")}</dt>
        <dd class="tabular-nums" data-testid="shift-opened-at">
          {formatDateTime(open.opened_at, locale.tz)}
        </dd>
      </div>
      <div>
        <dt class="type-hub-label">{t("hub.shift.opening_float")}</dt>
        <dd class="tabular-nums" data-testid="shift-float">
          {formatMoney(open.float_minor, locale)}
        </dd>
      </div>
      <div>
        <dt class="type-hub-label">{t("hub.shift.events")}</dt>
        <dd class="tabular-nums" data-testid="shift-events">{open.events}</dd>
      </div>
    </dl>
    <p class="note type-hub-label" data-testid="shift-blind">{t("hub.shift.blind")}</p>
    {#if opened}
      <p class="type-hub-label" role="status" data-testid="shift-status">{t("hub.shift.opened")}</p>
    {/if}
    <CashMovePanel
      {staff}
      ontimeout={() => void invalidate(SESSION_DEPENDENCY)}
      ondone={() => void invalidate(SNAPSHOT_DEPENDENCY)}
    />
  {:else if opening}
    <form class="form" onsubmit={confirm} novalidate data-testid="shift-form">
      <TextField
        label="hub.shift.float"
        bind:value={text}
        inputmode="decimal"
        autocomplete="off"
        {error}
      />
      <p class="note type-hub-label" data-testid="shift-carried">
        {t("hub.shift.float_hint", { amount: formatMoney(shift.carried_minor, locale) })}
      </p>
      <div class="actions">
        <Button label="hub.shift.confirm" type="submit" />
        <Button
          label="hub.shift.cancel"
          variant="secondary"
          onclick={() => {
            opening = false;
            error = null;
          }}
        />
      </div>
    </form>
  {:else}
    <p class="type-hub-body" data-testid="shift-none">{t("hub.shift.none")}</p>
    <div class="actions">
      <Button label="hub.shift.open" onclick={() => void begin()} />
    </div>
  {/if}

  {#if refusal}
    <p class="type-hub-label refusal" role="alert" data-testid="shift-refusal">
      <span aria-hidden="true">⚠</span>
      {refusal}
    </p>
  {/if}
</section>

<style>
  .shift,
  .form {
    display: grid;
    gap: var(--space-3);
    max-width: 32rem;
  }

  h1,
  h2,
  p,
  dl,
  dd {
    margin: 0;
  }

  .facts {
    display: grid;
    gap: var(--space-3);
  }

  .facts dt {
    color: var(--text-muted);
  }

  .note {
    color: var(--text-muted);
  }

  .refusal {
    color: var(--text-field-error-text);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
</style>
