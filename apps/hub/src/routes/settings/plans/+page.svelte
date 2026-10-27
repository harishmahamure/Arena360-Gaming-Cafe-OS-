<script lang="ts">
  import { Button, InlinePin, TextField, t } from "@arena/ui";
  import type { IntentPayload, PlanView } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, tick } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    elevate,
    hasPermission,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";
  import { branchLocale } from "../../../lib/locale.svelte";
  import { sessionStaff, snapshotOf } from "../../../lib/operating";
  import { clock, hoursText, loadPlans, parseClock, parseWhole } from "../../../lib/plans";
  import { parsePrice, priceText } from "../../../lib/products";

  type Job = { payload: IntentPayload; rev?: number; key: string };
  type Kind = "pack" | "block" | "pass";

  const KINDS: { id: Kind; label: MessageKey }[] = [
    { id: "pack", label: "hub.plans.kind.pack" },
    { id: "block", label: "hub.plans.kind.block" },
    { id: "pass", label: "hub.plans.kind.pass" },
  ];
  const INVALID: Record<string, MessageKey> = {
    name: "hub.plans.invalid.name",
    price_minor: "hub.plans.invalid.price",
    minutes: "hub.plans.invalid.hours",
    validity_days: "hub.plans.invalid.validity",
    window_start_min: "hub.plans.invalid.window",
    window_end_min: "hub.plans.invalid.window",
    group_ids: "hub.plans.invalid.groups",
  };
  const MAX_HOURS = 1666;
  const MAX_DAYS = 3650;
  const BLANK = {
    kind: "pack" as Kind,
    name: "",
    price: "",
    hours: "",
    days: "",
    start: "",
    end: "",
    groups: [] as string[],
    active: true,
  };

  let plans: PlanView[] = $state([]);
  let loading = $state(true);
  let loadFailed = $state(false);
  let editing: PlanView | null = $state(null);
  let form = $state({ ...BLANK, groups: [] as string[] });
  let errors: Record<string, MessageKey> = $state({});
  let busy = $state(false);
  let approving: Job | null = $state(null);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let formEl: HTMLFormElement | undefined = $state();

  const staff = $derived(sessionStaff(page.data));
  const needsApproval = $derived(!hasPermission(staff, "catalog.edit"));
  const groups = $derived(snapshotOf(page.data)?.groups ?? []);
  const exponent = $derived(branchLocale.current?.exponent ?? 2);
  const symbol = $derived(branchLocale.current?.symbol ?? "");
  const groupName = (id: string) => groups.find((g) => g.id === id)?.name ?? id;

  let loads = 0;
  let idem: { json: string; key: string } | null = null;

  function keyFor(payload: IntentPayload) {
    const json = JSON.stringify(payload);
    if (idem?.json !== json) idem = { json, key: newIdempotencyKey() };
    return idem.key;
  }

  async function load() {
    const mine = ++loads;
    const list = await loadPlans();
    if (mine !== loads) return;
    loading = false;
    if (list.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    loadFailed = list.kind !== "ok";
    if (list.kind === "ok") plans = list.plans;
  }

  onMount(() => void load());

  function reset(plan: PlanView | null = null) {
    editing = plan;
    errors = {};
    form = plan
      ? {
          kind: plan.plan_kind as Kind,
          name: plan.name,
          price: priceText(plan.price_minor, exponent),
          hours: plan.minutes ? String(plan.minutes / 60) : "",
          days: plan.validity_days ? String(plan.validity_days) : "",
          start: plan.window_start_min === undefined ? "" : clock(plan.window_start_min),
          end: plan.window_end_min === undefined ? "" : clock(plan.window_end_min),
          groups: [...plan.group_ids],
          active: plan.active,
        }
      : { ...BLANK, groups: [] };
    if (plan) void tick().then(() => formEl?.querySelector("input")?.focus());
  }

  async function run(job: Job, ticket?: string) {
    if (needsApproval && !ticket) {
      approvalPrompt = "ui.pin.prompt";
      approving = job;
      return;
    }
    if (busy) return;
    busy = true;
    status = null;
    errors = {};
    const outcome = await submitIntent(job.payload, job.key, job.rev, {
      elevationTicket: ticket,
    }).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      idem = null;
      status = { tone: "ok", message: "hub.settings.saved" };
      reset();
      return void (await load());
    }
    const err = outcome.kind === "error" ? outcome.error : null;
    if (err && err.code !== "PIN_REQUIRED") idem = null;
    if (err?.code === "PIN_REQUIRED") {
      approvalPrompt = "ui.pin.again";
      approving = job;
      return;
    }
    if (err?.code === "GROUP_KIND_MISMATCH") {
      return void (errors = { group_ids: "hub.plans.kind_mismatch" });
    }
    if (err?.code === "VALIDATION_FAILED" && err.field && INVALID[err.field]) {
      return void (errors = { [err.field]: INVALID[err.field] });
    }
    const stale =
      err?.code === "STALE" || (err?.code === "VALIDATION_FAILED" && err.field === "id");
    status = {
      tone: "refusal",
      message: stale
        ? "hub.plans.stale"
        : err?.code === "PERMISSION_DENIED"
          ? "hub.refusal.permission"
          : "hub.settings.failed",
    };
    if (stale) {
      await load();
      if (editing) reset(plans.find((p) => p.id === editing?.id) ?? null);
    }
  }

  async function approve(pin: string) {
    const job = approving;
    if (!job) return { ok: false, message: "ui.pin.failed" } as const;
    const outcome = await elevate([{ intent_kind: job.payload.kind }], pin);
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    approving = null;
    void run(job, outcome.ticket);
    return { ok: true } as const;
  }

  function save(event: SubmitEvent) {
    event.preventDefault();
    const bad: Record<string, MessageKey> = {};
    const price = parsePrice(form.price, exponent);
    if (price === null || price < 1) bad.price_minor = INVALID.price_minor;
    const hours = form.kind === "pack" ? parseHours(form.hours) : null;
    if (form.kind === "pack" && hours === null) bad.minutes = INVALID.minutes;
    const days = form.kind === "block" ? null : parseWhole(form.days, MAX_DAYS);
    if (form.kind !== "block" && days === null) bad.validity_days = INVALID.validity_days;
    const start = form.kind === "pack" ? null : parseClock(form.start);
    const end = form.kind === "pack" ? null : parseClock(form.end);
    if (form.kind !== "pack" && (start === null || end === null || start === end)) {
      bad.window_start_min = INVALID.window_start_min;
    }
    if (form.groups.length === 0) bad.group_ids = INVALID.group_ids;
    if (Object.keys(bad).length) {
      errors = bad;
      return void tick().then(() =>
        formEl?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus(),
      );
    }
    const payload: IntentPayload = {
      kind: "plan.upsert",
      id: editing?.id,
      plan_kind: form.kind,
      name: form.name,
      price_minor: price!,
      minutes: hours ?? undefined,
      validity_days: days ?? undefined,
      window_start_min: start ?? undefined,
      window_end_min: end ?? undefined,
      group_ids: groups.filter((g) => form.groups.includes(g.id)).map((g) => g.id),
      active: form.active,
    };
    void run({ payload, rev: editing?.rev, key: keyFor(payload) });
  }

  function parseHours(text: string): number | null {
    if (!/^\d{1,4}(\.\d{1,2})?$/.test(text.trim())) return null;
    const minutes = Number(text.trim()) * 60;
    return Number.isInteger(minutes) && minutes >= 1 && minutes <= MAX_HOURS * 60 ? minutes : null;
  }

  function toggle(id: string) {
    form.groups = form.groups.includes(id)
      ? form.groups.filter((g) => g !== id)
      : [...form.groups, id];
  }

  const money = (minor: number) => {
    const parts = [symbol, priceText(minor, exponent)];
    return (branchLocale.current?.symbol_position === "after" ? parts.reverse() : parts).join("");
  };

  function terms(p: PlanView): string {
    const days = p.validity_days ?? 0;
    if (p.plan_kind === "pack") {
      const { h, m } = hoursText(p.minutes ?? 0);
      return t("hub.plans.terms.pack", {
        time: m ? t("hub.plans.time_hours_minutes", { h, m }) : t("hub.plans.time_hours", { h }),
        days,
      });
    }
    const window = t("hub.plans.window", {
      start: clock(p.window_start_min ?? 0),
      end: clock(p.window_end_min ?? 0),
    });
    return p.plan_kind === "block"
      ? t("hub.plans.terms.block", { window })
      : t("hub.plans.terms.pass", { window, days });
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.settings.nav.plans") })}</title>
</svelte:head>

<section class="plans" aria-labelledby="plans-title">
  <h2 class="type-hub-title" id="plans-title">{t("hub.plans.title")}</h2>
  <p class="intro type-hub-body">{t("hub.plans.intro")}</p>

  <form class="form" bind:this={formEl} onsubmit={save} data-testid="plan-form">
    <fieldset class="field type-hub-label" disabled={editing !== null}>
      <legend>{t("hub.plans.kind")}</legend>
      <div class="kinds">
        {#each KINDS as k (k.id)}
          <label class="check type-hub-label"
            ><input type="radio" name="plan-kind" value={k.id} bind:group={form.kind} />{t(
              k.label,
            )}</label
          >
        {/each}
      </div>
    </fieldset>
    <TextField label="hub.plans.name" bind:value={form.name} error={errors.name} required />
    <TextField
      label="hub.plans.price"
      bind:value={form.price}
      error={errors.price_minor}
      inputmode="decimal"
      required
    />
    {#if form.kind === "pack"}
      <TextField
        label="hub.plans.hours"
        bind:value={form.hours}
        error={errors.minutes}
        inputmode="decimal"
        required
      />
    {/if}
    {#if form.kind !== "block"}
      <TextField
        label="hub.plans.validity"
        bind:value={form.days}
        error={errors.validity_days}
        inputmode="numeric"
        required
      />
    {/if}
    {#if form.kind !== "pack"}
      <div class="window">
        <label class="field type-hub-label">
          <span>{t(form.kind === "block" ? "hub.plans.window_start" : "hub.plans.play_start")}</span
          >
          <input
            class="ui-interactive time"
            type="time"
            bind:value={form.start}
            aria-invalid={errors.window_start_min ? "true" : undefined}
            aria-describedby={errors.window_start_min ? "plan-window-error" : undefined}
          />
        </label>
        <label class="field type-hub-label">
          <span>{t(form.kind === "block" ? "hub.plans.window_end" : "hub.plans.play_end")}</span>
          <input
            class="ui-interactive time"
            type="time"
            bind:value={form.end}
            aria-invalid={errors.window_start_min ? "true" : undefined}
          />
        </label>
      </div>
      {#if errors.window_start_min}<p class="error" id="plan-window-error" role="alert">
          {t(errors.window_start_min)}
        </p>{/if}
      {#if form.kind === "block"}
        <p class="hint type-hub-label">{t("hub.plans.block_hint")}</p>
      {:else}
        <p class="hint type-hub-label">{t("hub.plans.pass_hint")}</p>
      {/if}
    {/if}
    <fieldset class="field type-hub-label" data-testid="plan-groups">
      <legend>{t("hub.plans.groups")}</legend>
      {#each groups as g (g.id)}
        <label class="check type-hub-label"
          ><input
            type="checkbox"
            checked={form.groups.includes(g.id)}
            onchange={() => toggle(g.id)}
          />{g.name}</label
        >
      {:else}
        <p class="hint type-hub-label">{t("hub.plans.no_groups")}</p>
      {/each}
      {#if errors.group_ids}<p class="error" role="alert">{t(errors.group_ids)}</p>{/if}
    </fieldset>
    <label class="check type-hub-label"
      ><input type="checkbox" bind:checked={form.active} />{t("hub.plans.active")}</label
    >
    <div class="actions">
      <Button label={editing ? "hub.plans.save" : "hub.plans.add"} type="submit" />
      {#if editing}<Button
          label="hub.plans.cancel"
          variant="secondary"
          onclick={() => reset()}
        />{/if}
    </div>
  </form>

  <p
    class="status type-hub-label"
    class:refusal={status?.tone === "refusal"}
    role="status"
    data-testid="plan-status"
  >
    {#if status}
      {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(status.message)}</span>
    {/if}
  </p>

  {#if approving}
    <div class="approval" data-testid="plan-approval">
      <p class="type-hub-label">{t("hub.plans.approves")}</p>
      <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = null)} />
    </div>
  {/if}

  <h2 class="type-hub-title">{t("hub.plans.list")}</h2>
  {#if loadFailed}
    <p class="type-hub-body" role="alert">{t("hub.plans.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void load()} />
  {:else if loading}
    <p class="type-hub-body" aria-busy="true">…</p>
  {:else if plans.length === 0}
    <p class="type-hub-body">{t("hub.plans.empty")}</p>
  {:else}
    <ul class="rows" data-testid="plan-list">
      {#each plans as p (p.id)}
        <li class="row" data-testid="plan-row" data-plan-id={p.id}>
          <span class="type-hub-title">{p.name}</span>
          <span class="badge type-hub-label"
            >{t(KINDS.find((k) => k.id === p.plan_kind)?.label ?? "hub.plans.kind.pack")}</span
          >
          <span class="type-hub-label">{money(p.price_minor)}</span>
          <span class="type-hub-label" data-testid="plan-terms">{terms(p)}</span>
          <span class="type-hub-label"
            >{t("hub.plans.for_groups", {
              names: p.group_ids.map(groupName).join(", "),
            })}</span
          >
          {#if !p.active}<span class="badge type-hub-label">{t("hub.plans.badge.inactive")}</span
            >{/if}
          <div class="actions">
            <button type="button" class="ui-interactive btn type-hub-label" onclick={() => reset(p)}
              >{t("hub.plans.edit", { name: p.name })}</button
            >
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .plans,
  .form {
    display: grid;
    gap: var(--space-3);
    max-width: 48rem;
  }

  h2,
  p {
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

  fieldset {
    margin: 0;
    padding: 0;
    border: 0;
  }

  .kinds,
  .window {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .error {
    color: var(--text-field-error-text);
  }

  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
  }

  .check input {
    inline-size: var(--space-touch-min);
    block-size: var(--space-touch-min);
  }

  .time {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
  }

  .rows {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background-color: var(--surface);
  }

  .badge {
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-inline-start: auto;
  }

  .form .actions {
    margin-inline-start: 0;
  }

  .btn {
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--surface-2);
    color: var(--text);
    cursor: pointer;
  }

  .status {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }

  .approval {
    display: grid;
    gap: var(--space-2);
  }
</style>
