<script lang="ts">
  import { Button, InlinePin, TextField, t } from "@arena/ui";
  import type {
    CouponTarget,
    CouponView,
    IntentPayload,
    PlanView,
    Product,
  } from "@arena/ui/generated";
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
  import {
    codeValid,
    couponPercentInput,
    loadCoupons,
    normaliseCode,
    parseCouponPercent,
    usageText,
  } from "../../../lib/coupons";
  import { branchLocale } from "../../../lib/locale.svelte";
  import { sessionStaff, snapshotOf } from "../../../lib/operating";
  import { loadPlans, parseWhole } from "../../../lib/plans";
  import { loadProducts, parsePrice, priceText } from "../../../lib/products";

  type Job = { payload: IntentPayload; rev?: number; key: string };
  type Kind = "percent" | "amount";

  const INVALID: Record<string, MessageKey> = {
    code: "hub.coupons.invalid.code",
    percent_bp: "hub.coupons.invalid.percent",
    amount_minor: "hub.coupons.invalid.amount",
    valid_from: "hub.coupons.invalid.dates",
    valid_to: "hub.coupons.invalid.dates",
    usage_limit: "hub.coupons.invalid.limit",
    min_bill_minor: "hub.coupons.invalid.minimum",
    targets: "hub.coupons.invalid.targets",
  };
  const BLANK = {
    code: "",
    kind: "percent" as Kind,
    value: "",
    from: "",
    to: "",
    limit: "",
    minimum: "",
    groups: [] as string[],
    plans: [] as string[],
    products: [] as string[],
    active: true,
  };

  let coupons: CouponView[] = $state([]);
  let plans: PlanView[] = $state([]);
  let products: Product[] = $state([]);
  let loading = $state(true);
  let loadFailed = $state(false);
  let editing: CouponView | null = $state(null);
  let form = $state({
    ...BLANK,
    groups: [] as string[],
    plans: [] as string[],
    products: [] as string[],
  });
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

  let loads = 0;
  let idem: { json: string; key: string } | null = null;

  function keyFor(payload: IntentPayload) {
    const json = JSON.stringify(payload);
    if (idem?.json !== json) idem = { json, key: newIdempotencyKey() };
    return idem.key;
  }

  async function load() {
    const mine = ++loads;
    const [list, planList, productList] = await Promise.all([
      loadCoupons(),
      loadPlans(),
      loadProducts(),
    ]);
    if (mine !== loads) return;
    loading = false;
    if (list.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    loadFailed = list.kind !== "ok";
    if (list.kind === "ok") coupons = list.coupons;
    if (planList.kind === "ok") plans = planList.plans;
    if (productList.kind === "ok") products = productList.products;
  }

  onMount(() => void load());

  const idsOf = (c: CouponView, kind: string) =>
    c.targets.filter((x) => x.kind === kind).map((x) => x.id);

  function reset(coupon: CouponView | null = null) {
    editing = coupon;
    errors = {};
    form = coupon
      ? {
          code: coupon.code,
          kind: coupon.percent_bp == null ? "amount" : "percent",
          value:
            coupon.percent_bp == null
              ? priceText(coupon.amount_minor ?? 0, exponent)
              : couponPercentInput(coupon.percent_bp),
          from: coupon.valid_from ?? "",
          to: coupon.valid_to ?? "",
          limit: coupon.usage_limit == null ? "" : String(coupon.usage_limit),
          minimum: coupon.min_bill_minor > 0 ? priceText(coupon.min_bill_minor, exponent) : "",
          groups: idsOf(coupon, "group"),
          plans: idsOf(coupon, "plan"),
          products: idsOf(coupon, "product"),
          active: coupon.active,
        }
      : { ...BLANK, groups: [], plans: [], products: [] };
    if (coupon) void tick().then(() => formEl?.querySelector("input")?.focus());
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
        ? "hub.coupons.stale"
        : err?.code === "PERMISSION_DENIED"
          ? "hub.refusal.permission"
          : "hub.settings.failed",
    };
    if (stale) {
      await load();
      if (editing) reset(coupons.find((c) => c.id === editing?.id) ?? null);
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
    const code = normaliseCode(form.code);
    if (!codeValid(code)) bad.code = INVALID.code;
    const percent = form.kind === "percent" ? parseCouponPercent(form.value) : null;
    const amount = form.kind === "amount" ? parsePrice(form.value, exponent) : null;
    if (form.kind === "percent" && percent === null) bad.percent_bp = INVALID.percent_bp;
    if (form.kind === "amount" && (amount === null || amount < 1)) {
      bad.amount_minor = INVALID.amount_minor;
    }
    const from = form.from.trim();
    const to = form.to.trim();
    const date = /^\d{4}-\d{2}-\d{2}$/;
    if ((from && !date.test(from)) || (to && !date.test(to)) || (from && to && to < from)) {
      bad.valid_from = INVALID.valid_from;
    }
    const limit = form.limit.trim() === "" ? undefined : parseWhole(form.limit, 1_000_000_000);
    if (limit === null) bad.usage_limit = INVALID.usage_limit;
    const minimum = form.minimum.trim() === "" ? 0 : parsePrice(form.minimum, exponent);
    if (minimum === null) bad.min_bill_minor = INVALID.min_bill_minor;
    if (Object.keys(bad).length) {
      errors = bad;
      return void tick().then(() =>
        formEl?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus(),
      );
    }
    const targets: CouponTarget[] = [
      ...form.groups.map((id) => ({ kind: "group", id })),
      ...form.plans.map((id) => ({ kind: "plan", id })),
      ...form.products.map((id) => ({ kind: "product", id })),
    ];
    const payload: IntentPayload = {
      kind: "coupon.upsert",
      id: editing?.id,
      code,
      percent_bp: percent ?? undefined,
      amount_minor: amount ?? undefined,
      valid_from: from || undefined,
      valid_to: to || undefined,
      usage_limit: limit ?? undefined,
      min_bill_minor: minimum ?? 0,
      targets,
      active: form.active,
    };
    void run({ payload, rev: editing?.rev, key: keyFor(payload) });
  }

  function toggle(list: "groups" | "plans" | "products", id: string) {
    form[list] = form[list].includes(id) ? form[list].filter((x) => x !== id) : [...form[list], id];
  }

  const money = (minor: number) => {
    const parts = [symbol, priceText(minor, exponent)];
    return (branchLocale.current?.symbol_position === "after" ? parts.reverse() : parts).join("");
  };

  const percentOff = (bp: number) => `${couponPercentInput(bp)}%`; // i18n-ignore (a number)

  const offText = (c: CouponView) =>
    c.percent_bp == null
      ? t("hub.coupons.off_amount", { amount: money(c.amount_minor ?? 0) })
      : t("hub.coupons.off_percent", { percent: percentOff(c.percent_bp) });

  const windowText = (c: CouponView) =>
    c.valid_from && c.valid_to
      ? t("hub.coupons.window.both", { from: c.valid_from, to: c.valid_to })
      : c.valid_from
        ? t("hub.coupons.window.from", { date: c.valid_from })
        : c.valid_to
          ? t("hub.coupons.window.to", { date: c.valid_to })
          : t("hub.coupons.window.always");

  const nameOf = (c: CouponView) => {
    const names = c.targets.map((x) =>
      x.kind === "group"
        ? (groups.find((g) => g.id === x.id)?.name ?? x.id)
        : x.kind === "plan"
          ? (plans.find((p) => p.id === x.id)?.name ?? x.id)
          : (products.find((p) => p.id === x.id)?.name ?? x.id),
    );
    return names.length === 0 ? t("hub.coupons.targets.all") : names.join(", ");
  };
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.settings.nav.coupons") })}</title>
</svelte:head>

<section class="coupons" aria-labelledby="coupons-title">
  <h2 class="type-hub-title" id="coupons-title">{t("hub.coupons.title")}</h2>
  <p class="intro type-hub-body">{t("hub.coupons.intro")}</p>

  <form class="form" bind:this={formEl} onsubmit={save} data-testid="coupon-form">
    <TextField label="hub.coupons.code" bind:value={form.code} error={errors.code} required />
    <fieldset class="field type-hub-label">
      <legend>{t("hub.coupons.kind")}</legend>
      <div class="inline">
        <label class="check type-hub-label"
          ><input type="radio" name="coupon-kind" value="percent" bind:group={form.kind} />{t(
            "hub.coupons.kind.percent",
          )}</label
        >
        <label class="check type-hub-label"
          ><input type="radio" name="coupon-kind" value="amount" bind:group={form.kind} />{t(
            "hub.coupons.kind.amount",
          )}</label
        >
      </div>
    </fieldset>
    <TextField
      label={form.kind === "percent" ? "hub.coupons.percent" : "hub.coupons.amount"}
      bind:value={form.value}
      error={form.kind === "percent" ? errors.percent_bp : errors.amount_minor}
      inputmode="decimal"
      required
    />
    <div class="inline">
      <label class="field type-hub-label">
        <span>{t("hub.coupons.from")}</span>
        <input
          class="ui-interactive time"
          type="date"
          bind:value={form.from}
          aria-invalid={errors.valid_from ? "true" : undefined}
        />
      </label>
      <label class="field type-hub-label">
        <span>{t("hub.coupons.to")}</span>
        <input
          class="ui-interactive time"
          type="date"
          bind:value={form.to}
          aria-invalid={errors.valid_from ? "true" : undefined}
        />
      </label>
    </div>
    {#if errors.valid_from}<p class="error" role="alert">{t(errors.valid_from)}</p>{/if}
    <p class="hint type-hub-label">{t("hub.coupons.dates_hint")}</p>
    <TextField
      label="hub.coupons.limit"
      bind:value={form.limit}
      error={errors.usage_limit}
      inputmode="numeric"
    />
    <TextField
      label="hub.coupons.minimum"
      bind:value={form.minimum}
      error={errors.min_bill_minor}
      inputmode="decimal"
    />

    <fieldset class="field type-hub-label" data-testid="coupon-targets">
      <legend>{t("hub.coupons.targets")}</legend>
      <p class="hint type-hub-label">{t("hub.coupons.targets_hint")}</p>
      {#if groups.length > 0}
        <p class="type-hub-label">{t("hub.coupons.targets.groups")}</p>
        {#each groups as g (g.id)}
          <label class="check type-hub-label"
            ><input
              type="checkbox"
              checked={form.groups.includes(g.id)}
              onchange={() => toggle("groups", g.id)}
            />{g.name}</label
          >
        {/each}
      {/if}
      {#if plans.length > 0}
        <p class="type-hub-label">{t("hub.coupons.targets.plans")}</p>
        {#each plans as p (p.id)}
          <label class="check type-hub-label"
            ><input
              type="checkbox"
              checked={form.plans.includes(p.id)}
              onchange={() => toggle("plans", p.id)}
            />{p.name}</label
          >
        {/each}
      {/if}
      {#if products.length > 0}
        <p class="type-hub-label">{t("hub.coupons.targets.products")}</p>
        <div class="scroll">
          {#each products as p (p.id)}
            <label class="check type-hub-label"
              ><input
                type="checkbox"
                checked={form.products.includes(p.id)}
                onchange={() => toggle("products", p.id)}
              />{p.name}</label
            >
          {/each}
        </div>
      {/if}
      {#if errors.targets}<p class="error" role="alert">{t(errors.targets)}</p>{/if}
    </fieldset>

    <label class="check type-hub-label"
      ><input type="checkbox" bind:checked={form.active} />{t("hub.coupons.active")}</label
    >
    <div class="actions">
      <Button label={editing ? "hub.coupons.save" : "hub.coupons.add"} type="submit" />
      {#if editing}<Button
          label="hub.coupons.cancel"
          variant="secondary"
          onclick={() => reset()}
        />{/if}
    </div>
  </form>

  <p
    class="status type-hub-label"
    class:refusal={status?.tone === "refusal"}
    role="status"
    data-testid="coupon-status"
  >
    {#if status}
      {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(status.message)}</span>
    {/if}
  </p>

  {#if approving}
    <div class="approval" data-testid="coupon-approval">
      <p class="type-hub-label">{t("hub.coupons.approves")}</p>
      <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = null)} />
    </div>
  {/if}

  <h2 class="type-hub-title">{t("hub.coupons.list")}</h2>
  {#if loadFailed}
    <p class="type-hub-body" role="alert">{t("hub.coupons.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void load()} />
  {:else if loading}
    <p class="type-hub-body" aria-busy="true">…</p>
  {:else if coupons.length === 0}
    <p class="type-hub-body">{t("hub.coupons.empty")}</p>
  {:else}
    <ul class="rows" data-testid="coupon-list">
      {#each coupons as c (c.id)}
        <li class="row" data-testid="coupon-row" data-coupon-id={c.id}>
          <span class="type-hub-title" data-testid="coupon-code">{c.code}</span>
          <span class="type-hub-label" data-testid="coupon-off">{offText(c)}</span>
          <span class="type-hub-label" data-testid="coupon-window">{windowText(c)}</span>
          <span class="type-hub-label" data-testid="coupon-used">{usageText(c)}</span>
          {#if c.min_bill_minor > 0}
            <span class="type-hub-label"
              >{t("hub.coupons.min_text", { amount: money(c.min_bill_minor) })}</span
            >
          {/if}
          <span class="type-hub-label" data-testid="coupon-targets-text">{nameOf(c)}</span>
          {#if !c.active}<span class="badge type-hub-label">{t("hub.coupons.badge.inactive")}</span
            >{/if}
          <div class="actions">
            <button type="button" class="ui-interactive btn type-hub-label" onclick={() => reset(c)}
              >{t("hub.coupons.edit", { code: c.code })}</button
            >
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .coupons,
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

  .scroll {
    display: grid;
    max-block-size: 16rem;
    overflow-y: auto;
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
