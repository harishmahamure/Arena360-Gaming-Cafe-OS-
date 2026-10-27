<script lang="ts">
  import { Button, InlinePin, TextField, t } from "@arena/ui";
  import type { IntentPayload, RuleView } from "@arena/ui/generated";
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
  import { clock, parseClock, parseWhole } from "../../../lib/plans";
  import { parsePrice, priceText } from "../../../lib/products";
  import {
    DAYS,
    bitsOf,
    loadRules,
    maskOf,
    parsePercent,
    percentInput,
    percentText,
    scheduleText,
  } from "../../../lib/rules";

  type Job = { payload: IntentPayload; rev?: number; key: string };
  type Kind = "weekly" | "date";
  type Adjust = "percent_bp" | "override_minor";

  const INVALID: Record<string, MessageKey> = {
    name: "hub.rules.invalid.name",
    group_id: "hub.rules.invalid.group",
    days_mask: "hub.rules.invalid.days",
    start_min: "hub.rules.invalid.window",
    end_min: "hub.rules.invalid.window",
    date_from: "hub.rules.invalid.dates",
    date_to: "hub.rules.invalid.dates",
    adjust_value: "hub.rules.invalid.adjust",
    priority: "hub.rules.invalid.priority",
  };
  const BLANK = {
    kind: "weekly" as Kind,
    name: "",
    group: "",
    days: [] as number[],
    start: "",
    end: "",
    from: "",
    to: "",
    adjust: "percent_bp" as Adjust,
    value: "",
    priority: "0",
    active: true,
  };

  let rules: RuleView[] = $state([]);
  let loading = $state(true);
  let loadFailed = $state(false);
  let editing: RuleView | null = $state(null);
  let form = $state({ ...BLANK, days: [] as number[] });
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
    const list = await loadRules();
    if (mine !== loads) return;
    loading = false;
    if (list.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    loadFailed = list.kind !== "ok";
    if (list.kind === "ok") rules = list.rules;
  }

  onMount(() => void load());

  function reset(rule: RuleView | null = null) {
    editing = rule;
    errors = {};
    form = rule
      ? {
          kind: rule.rule_kind as Kind,
          name: rule.name,
          group: rule.group_id,
          days: bitsOf(rule.days_mask ?? 0),
          start: rule.start_min === undefined ? "" : clock(rule.start_min),
          end: rule.end_min === undefined ? "" : clock(rule.end_min),
          from: rule.date_from ?? "",
          to: rule.date_to && rule.date_to !== rule.date_from ? rule.date_to : "",
          adjust: rule.adjust_kind as Adjust,
          value:
            rule.adjust_kind === "percent_bp"
              ? percentInput(rule.adjust_value)
              : priceText(rule.adjust_value, exponent),
          priority: String(rule.priority),
          active: rule.active,
        }
      : { ...BLANK, days: [] };
    if (rule) void tick().then(() => formEl?.querySelector("input")?.focus());
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
    if (err?.code === "VALIDATION_FAILED" && err.field && INVALID[err.field]) {
      return void (errors = { [err.field]: INVALID[err.field] });
    }
    const stale =
      err?.code === "STALE" || (err?.code === "VALIDATION_FAILED" && err.field === "id");
    status = {
      tone: "refusal",
      message: stale
        ? "hub.rules.stale"
        : err?.code === "PERMISSION_DENIED"
          ? "hub.refusal.permission"
          : "hub.settings.failed",
    };
    if (stale) {
      await load();
      if (editing) reset(rules.find((r) => r.id === editing?.id) ?? null);
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
    if (!form.name.trim()) bad.name = INVALID.name;
    if (!groups.some((g) => g.id === form.group)) bad.group_id = INVALID.group_id;
    const weekly = form.kind === "weekly";
    if (weekly && form.days.length === 0) bad.days_mask = INVALID.days_mask;
    const start = weekly ? parseClock(form.start) : null;
    const end = weekly ? parseClock(form.end) : null;
    if (weekly && (start === null || end === null || start === end)) {
      bad.start_min = INVALID.start_min;
    }
    const from = form.from.trim();
    const to = form.to.trim() || from;
    if (!weekly && (!/^\d{4}-\d{2}-\d{2}$/.test(from) || !/^\d{4}-\d{2}-\d{2}$/.test(to))) {
      bad.date_from = INVALID.date_from;
    } else if (!weekly && to < from) {
      bad.date_to = INVALID.date_to;
    }
    const value =
      form.adjust === "percent_bp" ? parsePercent(form.value) : parsePrice(form.value, exponent);
    if (value === null || (form.adjust === "percent_bp" ? value < -10_000 : value < 1)) {
      bad.adjust_value = INVALID.adjust_value;
    }
    const priority = form.priority.trim() === "0" ? 0 : parseWhole(form.priority, 1000);
    if (priority === null) bad.priority = INVALID.priority;
    if (Object.keys(bad).length) {
      errors = bad;
      return void tick().then(() =>
        formEl?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus(),
      );
    }
    const payload: IntentPayload = {
      kind: "rule.upsert",
      id: editing?.id,
      name: form.name,
      group_id: form.group,
      rule_kind: form.kind,
      days_mask: weekly ? maskOf(form.days) : undefined,
      start_min: start ?? undefined,
      end_min: end ?? undefined,
      date_from: weekly ? undefined : from,
      date_to: weekly ? undefined : to,
      adjust_kind: form.adjust,
      adjust_value: value!,
      priority: priority!,
      active: form.active,
    };
    void run({ payload, rev: editing?.rev, key: keyFor(payload) });
  }

  function toggleDay(bit: number) {
    form.days = form.days.includes(bit) ? form.days.filter((d) => d !== bit) : [...form.days, bit];
  }

  const money = (minor: number) => {
    const parts = [symbol, priceText(minor, exponent)];
    return (branchLocale.current?.symbol_position === "after" ? parts.reverse() : parts).join("");
  };

  const adjustText = (r: RuleView) =>
    r.adjust_kind === "percent_bp"
      ? percentText(r.adjust_value)
      : t("hub.rules.rate_per_hour", { rate: money(r.adjust_value) });
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.settings.nav.rules") })}</title>
</svelte:head>

<section class="rules" aria-labelledby="rules-title">
  <h2 class="type-hub-title" id="rules-title">{t("hub.rules.title")}</h2>
  <p class="intro type-hub-body">{t("hub.rules.intro")}</p>

  <form class="form" bind:this={formEl} onsubmit={save} data-testid="rule-form">
    <fieldset class="field type-hub-label">
      <legend>{t("hub.rules.type")}</legend>
      <div class="inline">
        <label class="check type-hub-label"
          ><input type="radio" name="rule-kind" value="weekly" bind:group={form.kind} />{t(
            "hub.rules.type.weekly",
          )}</label
        >
        <label class="check type-hub-label"
          ><input type="radio" name="rule-kind" value="date" bind:group={form.kind} />{t(
            "hub.rules.type.date",
          )}</label
        >
      </div>
    </fieldset>
    <TextField label="hub.rules.name" bind:value={form.name} error={errors.name} required />
    <fieldset class="field type-hub-label" data-testid="rule-groups">
      <legend>{t("hub.rules.group")}</legend>
      {#each groups as g (g.id)}
        <label class="check type-hub-label"
          ><input
            type="radio"
            name="rule-group"
            value={g.id}
            bind:group={form.group}
          />{g.name}</label
        >
      {:else}
        <p class="hint type-hub-label">{t("hub.rules.no_groups")}</p>
      {/each}
      {#if errors.group_id}<p class="error" role="alert">{t(errors.group_id)}</p>{/if}
    </fieldset>
    {#if form.kind === "weekly"}
      <fieldset class="field type-hub-label" data-testid="rule-days">
        <legend>{t("hub.rules.days")}</legend>
        <div class="inline">
          {#each DAYS as d (d.bit)}
            <label class="check type-hub-label"
              ><input
                type="checkbox"
                checked={form.days.includes(d.bit)}
                onchange={() => toggleDay(d.bit)}
              />{t(d.label)}</label
            >
          {/each}
        </div>
        {#if errors.days_mask}<p class="error" role="alert">{t(errors.days_mask)}</p>{/if}
      </fieldset>
      <div class="inline">
        <label class="field type-hub-label">
          <span>{t("hub.rules.start")}</span>
          <input
            class="ui-interactive time"
            type="time"
            bind:value={form.start}
            aria-invalid={errors.start_min ? "true" : undefined}
          />
        </label>
        <label class="field type-hub-label">
          <span>{t("hub.rules.end")}</span>
          <input
            class="ui-interactive time"
            type="time"
            bind:value={form.end}
            aria-invalid={errors.start_min ? "true" : undefined}
          />
        </label>
      </div>
      {#if errors.start_min}<p class="error" role="alert">{t(errors.start_min)}</p>{/if}
      <p class="hint type-hub-label">{t("hub.rules.window_hint")}</p>
    {:else}
      <div class="inline">
        <label class="field type-hub-label">
          <span>{t("hub.rules.from")}</span>
          <input
            class="ui-interactive time"
            type="date"
            bind:value={form.from}
            aria-invalid={errors.date_from ? "true" : undefined}
          />
        </label>
        <label class="field type-hub-label">
          <span>{t("hub.rules.to")}</span>
          <input
            class="ui-interactive time"
            type="date"
            bind:value={form.to}
            aria-invalid={errors.date_to ? "true" : undefined}
          />
        </label>
      </div>
      {#if errors.date_from}<p class="error" role="alert">{t(errors.date_from)}</p>{/if}
      {#if errors.date_to}<p class="error" role="alert">{t(errors.date_to)}</p>{/if}
      <p class="hint type-hub-label">{t("hub.rules.date_hint")}</p>
    {/if}
    <fieldset class="field type-hub-label">
      <legend>{t("hub.rules.adjust")}</legend>
      <div class="inline">
        <label class="check type-hub-label"
          ><input type="radio" name="rule-adjust" value="percent_bp" bind:group={form.adjust} />{t(
            "hub.rules.adjust.percent",
          )}</label
        >
        <label class="check type-hub-label"
          ><input
            type="radio"
            name="rule-adjust"
            value="override_minor"
            bind:group={form.adjust}
          />{t("hub.rules.adjust.override")}</label
        >
      </div>
    </fieldset>
    <TextField
      label={form.adjust === "percent_bp" ? "hub.rules.percent" : "hub.rules.rate"}
      bind:value={form.value}
      error={errors.adjust_value}
      inputmode="decimal"
      required
    />
    <TextField
      label="hub.rules.priority"
      bind:value={form.priority}
      error={errors.priority}
      inputmode="numeric"
    />
    <label class="check type-hub-label"
      ><input type="checkbox" bind:checked={form.active} />{t("hub.rules.active")}</label
    >
    <div class="actions">
      <Button label={editing ? "hub.rules.save" : "hub.rules.add"} type="submit" />
      {#if editing}<Button
          label="hub.rules.cancel"
          variant="secondary"
          onclick={() => reset()}
        />{/if}
    </div>
  </form>

  <p
    class="status type-hub-label"
    class:refusal={status?.tone === "refusal"}
    role="status"
    data-testid="rule-status"
  >
    {#if status}
      {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(status.message)}</span>
    {/if}
  </p>

  {#if approving}
    <div class="approval" data-testid="rule-approval">
      <p class="type-hub-label">{t("hub.rules.approves")}</p>
      <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = null)} />
    </div>
  {/if}

  <h2 class="type-hub-title">{t("hub.rules.list")}</h2>
  {#if loadFailed}
    <p class="type-hub-body" role="alert">{t("hub.rules.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void load()} />
  {:else if loading}
    <p class="type-hub-body" aria-busy="true">…</p>
  {:else if rules.length === 0}
    <p class="type-hub-body">{t("hub.rules.empty")}</p>
  {:else}
    <ul class="rows" data-testid="rule-list">
      {#each rules as r (r.id)}
        <li class="row" data-testid="rule-row" data-rule-id={r.id}>
          <span class="type-hub-title">{r.name}</span>
          <span class="badge type-hub-label"
            >{t(r.rule_kind === "weekly" ? "hub.rules.type.weekly" : "hub.rules.type.date")}</span
          >
          <span class="type-hub-label"
            >{t("hub.rules.for_group", { name: groupName(r.group_id) })}</span
          >
          <span class="type-hub-label" data-testid="rule-schedule">{scheduleText(r)}</span>
          <span class="type-hub-label" data-testid="rule-adjust">{adjustText(r)}</span>
          <span class="type-hub-label">{t("hub.rules.priority_text", { n: r.priority })}</span>
          {#if !r.active}<span class="badge type-hub-label">{t("hub.rules.badge.inactive")}</span
            >{/if}
          <div class="actions">
            <button type="button" class="ui-interactive btn type-hub-label" onclick={() => reset(r)}
              >{t("hub.rules.edit", { name: r.name })}</button
            >
          </div>
          {#if r.conflicts.length}
            <ul class="conflicts" data-testid="rule-conflicts">
              {#each r.conflicts as c (c.rule_id)}
                <li class="conflict type-hub-label">
                  <span aria-hidden="true">⚠</span>
                  <span
                    >{t(
                      c.other_wins
                        ? "hub.rules.conflict.other_wins"
                        : "hub.rules.conflict.this_wins",
                      {
                        name: c.name,
                      },
                    )}</span
                  >
                </li>
              {/each}
            </ul>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .rules,
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

  .inline {
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

  .rows,
  .conflicts {
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

  .conflicts {
    flex-basis: 100%;
    gap: var(--space-1);
  }

  .conflict {
    display: flex;
    gap: var(--space-2);
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
