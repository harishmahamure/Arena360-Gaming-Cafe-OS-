<script lang="ts">
  import { Button, InlinePin, TextField, reasonOptions, t } from "@arena/ui";
  import type { IntentPayload, Product, ReasonCode } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onDestroy, onMount, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    dropElevation,
    elevate,
    hasPermission,
    newIdempotencyKey,
    submitIntent,
  } from "../../../lib/auth";
  import { catalog } from "../../../lib/catalog.svelte";
  import { sessionStaff } from "../../../lib/operating";
  import { loadProducts } from "../../../lib/products";

  type Job = { payload: IntentPayload; reason: ReasonCode; key: string };

  const UNDO_MS = 8000;
  const REASONS = reasonOptions("stock.waste");
  const INVALID: Record<string, MessageKey> = {
    product_id: "hub.waste.invalid.product",
    qty: "hub.waste.invalid.qty",
    reason: "hub.waste.invalid.reason",
  };

  let products: Product[] = $state([]);
  let loading = $state(true);
  let loadFailed = $state(false);
  let form = $state({ product: "", qty: "", reason: "" });
  let errors: Record<string, MessageKey> = $state({});
  let busy = $state(false);
  let approving: Job | null = $state(null);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let undoable: { moveId: string; ticket?: string } | null = $state(null);
  let left = $state(0);
  let timer: ReturnType<typeof setInterval> | undefined;
  let loads = 0;

  const staff = $derived(sessionStaff(page.data));
  const needsApproval = $derived(!hasPermission(staff, "stock.adjust"));
  const tracked = $derived(products.filter((p) => p.track_stock && !p.bundle));

  async function load() {
    const mine = ++loads;
    const list = await loadProducts();
    if (mine !== loads) return;
    loading = false;
    if (list.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    loadFailed = list.kind !== "ok";
    if (list.kind === "ok") products = list.products;
  }

  function stop(drop = true) {
    clearInterval(timer);
    if (drop && undoable?.ticket) void dropElevation(undoable.ticket);
    undoable = null;
  }

  function arm(moveId: string, ticket?: string) {
    stop();
    const until = Date.now() + UNDO_MS;
    undoable = { moveId, ticket };
    left = Math.ceil(UNDO_MS / 1000);
    timer = setInterval(() => {
      const ms = until - Date.now();
      if (ms <= 0) return stop();
      left = Math.ceil(ms / 1000);
    }, 250);
  }

  onMount(() => void load());
  onDestroy(() => stop());
  $effect(() => {
    if (catalog.version) untrack(() => void load());
  });

  async function fail(outcome: Awaited<ReturnType<typeof submitIntent>>) {
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    const err = outcome.kind === "error" ? outcome.error : null;
    if (err?.code === "OUT_OF_STOCK") return void (errors = { qty: "hub.waste.out_of_stock" });
    if (err?.code === "REASON_REQUIRED") return void (errors = { reason: INVALID.reason });
    if (err?.code === "VALIDATION_FAILED" && err.field && INVALID[err.field]) {
      return void (errors = { [err.field]: INVALID[err.field] });
    }
    status = {
      tone: "refusal",
      message:
        err?.code === "EXPIRED"
          ? "hub.waste.undo_expired"
          : err?.code === "STALE"
            ? "hub.waste.undo_stale"
            : err?.code === "PERMISSION_DENIED"
              ? "hub.refusal.permission"
              : "hub.settings.failed",
    };
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
    const outcome = await submitIntent(job.payload, job.key, undefined, {
      elevationTicket: ticket,
      reason: job.reason,
    }).finally(() => (busy = false));
    if (outcome.kind === "error" && outcome.error.code === "PIN_REQUIRED") {
      approvalPrompt = "ui.pin.again";
      approving = job;
      return;
    }
    if (outcome.kind !== "applied") {
      if (ticket) void dropElevation(ticket);
      return void (await fail(outcome));
    }
    status = { tone: "ok", message: "hub.waste.recorded" };
    const move = outcome.result.applied.changes.find((c) => c.tbl === "stock_moves");
    if (move) arm(move.row_id, ticket);
    else if (ticket) void dropElevation(ticket);
    form = { ...form, qty: "" };
    await load();
  }

  async function approve(pin: string) {
    const job = approving;
    if (!job) return { ok: false, message: "ui.pin.failed" } as const;
    // One PIN covers the waste and its undo, so Undo needs no second approval.
    const outcome = await elevate(
      [{ intent_kind: "stock.waste" }, { intent_kind: "stock.waste.reverted" }],
      pin,
    );
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
    if (busy) return;
    const qty = /^\d{1,7}$/.test(form.qty.trim()) ? Number(form.qty.trim()) : 0;
    const bad: Record<string, MessageKey> = {};
    if (!form.product) bad.product_id = INVALID.product_id;
    if (qty < 1) bad.qty = INVALID.qty;
    if (!form.reason) bad.reason = INVALID.reason;
    errors = bad;
    status = null;
    if (Object.keys(bad).length) return;
    void run({
      payload: { kind: "stock.waste", product_id: form.product, qty },
      reason: form.reason as ReasonCode,
      key: newIdempotencyKey(),
    });
  }

  async function undo() {
    if (!undoable || busy) return;
    const { moveId, ticket } = undoable;
    busy = true;
    status = null;
    const outcome = await submitIntent(
      { kind: "stock.waste.reverted", move_id: moveId },
      newIdempotencyKey(),
      undefined,
      { elevationTicket: ticket },
    ).finally(() => (busy = false));
    stop(false);
    if (outcome.kind !== "applied") return void (await fail(outcome));
    status = { tone: "ok", message: "hub.waste.undone" };
    await load();
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.waste.title") })}</title>
</svelte:head>

<section class="stock" aria-labelledby="waste-title">
  <h1 class="type-hub-numeric" id="waste-title">{t("hub.nav.inventory")}</h1>
  <h2 class="type-hub-title">{t("hub.waste.title")}</h2>
  <p class="intro type-hub-body">{t("hub.waste.intro")}</p>
  <a class="link type-hub-label" href="/inventory">{t("hub.stock.back")}</a>

  <form class="form" onsubmit={save} data-testid="waste-form">
    <div class="field type-hub-label">
      <label for="waste-product">{t("hub.stock.product")}</label>
      <select
        id="waste-product"
        class="ui-interactive select"
        bind:value={form.product}
        aria-invalid={errors.product_id ? "true" : undefined}
        aria-describedby={errors.product_id ? "waste-product-error" : undefined}
      >
        <option value="">{t("hub.stock.product_pick")}</option>
        {#each tracked as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
      </select>
      {#if errors.product_id}<p class="error" id="waste-product-error" role="alert">
          {t(errors.product_id)}
        </p>{/if}
    </div>
    <TextField label="hub.stock.qty" bind:value={form.qty} error={errors.qty} inputmode="numeric" />
    <div class="field type-hub-label">
      <label for="waste-reason">{t("hub.waste.reason")}</label>
      <select
        id="waste-reason"
        class="ui-interactive select"
        bind:value={form.reason}
        aria-invalid={errors.reason ? "true" : undefined}
        aria-describedby={errors.reason ? "waste-reason-error" : undefined}
      >
        <option value="">{t("hub.waste.reason_pick")}</option>
        {#each REASONS as r (r.value)}<option value={r.value}>{t(r.label)}</option>{/each}
      </select>
      {#if errors.reason}<p class="error" id="waste-reason-error" role="alert">
          {t(errors.reason)}
        </p>{/if}
    </div>
    <div class="actions">
      <Button label="hub.waste.submit" type="submit" />
    </div>
  </form>

  {#if approving}
    <div class="approval" data-testid="waste-approval">
      <p class="type-hub-label">{t("hub.waste.approves")}</p>
      <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = null)} />
    </div>
  {/if}

  <div
    class="status type-hub-label"
    class:refusal={status?.tone === "refusal"}
    role="status"
    data-testid="waste-status"
  >
    {#if status}
      {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(status.message)}</span>
    {/if}
    {#if undoable}
      <button
        type="button"
        class="ui-interactive btn type-hub-label"
        data-testid="waste-undo"
        onclick={() => void undo()}>{t("hub.stock.undo", { s: left })}</button
      >
    {/if}
  </div>

  <h2 class="type-hub-title">{t("hub.stock.on_hand")}</h2>
  {#if loadFailed}
    <p class="type-hub-body" role="alert">{t("hub.products.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void load()} />
  {:else if loading}
    <p class="type-hub-body" aria-busy="true">…</p>
  {:else if tracked.length === 0}
    <p class="type-hub-body">{t("hub.stock.empty")}</p>
  {:else}
    <ul class="rows" data-testid="waste-list">
      {#each tracked as p (p.id)}
        <li class="row" data-testid="waste-row" data-product-id={p.id}>
          <span class="type-hub-title">{p.name}</span>
          <span class="type-hub-label">{t("hub.waste.row", { qty: p.quantity })}</span>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .stock,
  .form {
    display: grid;
    gap: var(--space-3);
    max-width: 48rem;
  }

  h1,
  h2,
  p {
    margin: 0;
  }

  .intro {
    color: var(--text-muted);
  }

  .link {
    color: var(--text);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  .error {
    color: var(--text-field-error-text);
  }

  .select {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
  }

  .approval {
    display: grid;
    gap: var(--space-2);
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
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    min-height: 1lh;
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }
</style>
