<script lang="ts">
  import { Button, TextField, t } from "@arena/ui";
  import type { IntentPayload, Product } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onDestroy, onMount, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, newIdempotencyKey, submitIntent } from "../../../lib/auth";
  import { catalog } from "../../../lib/catalog.svelte";
  import { branchLocale } from "../../../lib/locale.svelte";
  import { loadProducts, parsePrice, priceText } from "../../../lib/products";

  const UNDO_MS = 8000;
  const INVALID: Record<string, MessageKey> = {
    product_id: "hub.stock.invalid.product",
    qty: "hub.stock.invalid.qty",
    unit_cost_minor: "hub.stock.invalid.cost",
    supplier: "hub.stock.invalid.supplier",
    expiry: "hub.stock.invalid.expiry",
  };

  let products: Product[] = $state([]);
  let loading = $state(true);
  let loadFailed = $state(false);
  let form = $state({ product: "", qty: "", cost: "", supplier: "", expiry: "" });
  let errors: Record<string, MessageKey> = $state({});
  let busy = $state(false);
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let undoable: { moveId: string; until: number } | null = $state(null);
  let left = $state(0);
  let timer: ReturnType<typeof setInterval> | undefined;
  let loads = 0;

  const exponent = $derived(branchLocale.current?.exponent ?? 2);
  const symbol = $derived(branchLocale.current?.symbol ?? "");
  const tracked = $derived(products.filter((p) => p.track_stock && !p.bundle));

  const money = (minor: number) => {
    const parts = [symbol, priceText(minor, exponent)];
    return (branchLocale.current?.symbol_position === "after" ? parts.reverse() : parts).join("");
  };

  async function load() {
    const mine = ++loads;
    const list = await loadProducts();
    if (mine !== loads) return;
    loading = false;
    if (list.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    loadFailed = list.kind !== "ok";
    if (list.kind === "ok") products = list.products;
  }

  function stop() {
    clearInterval(timer);
    undoable = null;
  }

  function arm(moveId: string) {
    stop();
    const until = Date.now() + UNDO_MS;
    undoable = { moveId, until };
    left = Math.ceil(UNDO_MS / 1000);
    timer = setInterval(() => {
      const ms = until - Date.now();
      if (ms <= 0) return stop();
      left = Math.ceil(ms / 1000);
    }, 250);
  }

  onMount(() => void load());
  onDestroy(() => clearInterval(timer));
  $effect(() => {
    if (catalog.version) untrack(() => void load());
  });

  async function fail(outcome: Awaited<ReturnType<typeof submitIntent>>) {
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    const err = outcome.kind === "error" ? outcome.error : null;
    if (err?.code === "VALIDATION_FAILED" && err.field && INVALID[err.field]) {
      return void (errors = { [err.field]: INVALID[err.field] });
    }
    status = {
      tone: "refusal",
      message:
        err?.code === "EXPIRED"
          ? "hub.stock.undo_expired"
          : err?.code === "STALE"
            ? "hub.stock.undo_stale"
            : err?.code === "PERMISSION_DENIED"
              ? "hub.refusal.permission"
              : "hub.settings.failed",
    };
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (busy) return;
    const qty = /^\d{1,7}$/.test(form.qty.trim()) ? Number(form.qty.trim()) : 0;
    const cost = parsePrice(form.cost, exponent);
    const bad: Record<string, MessageKey> = {};
    if (!form.product) bad.product_id = INVALID.product_id;
    if (qty < 1) bad.qty = INVALID.qty;
    if (cost === null) bad.unit_cost_minor = INVALID.unit_cost_minor;
    errors = bad;
    status = null;
    if (Object.keys(bad).length) return;
    const payload: IntentPayload = {
      kind: "stock.in",
      product_id: form.product,
      qty,
      unit_cost_minor: cost!,
      supplier: form.supplier,
      expiry: form.expiry.trim() || undefined,
    };
    busy = true;
    const outcome = await submitIntent(payload, newIdempotencyKey()).finally(() => (busy = false));
    if (outcome.kind !== "applied") return void (await fail(outcome));
    status = { tone: "ok", message: "hub.stock.recorded" };
    const move = outcome.result.applied.changes.find((c) => c.tbl === "stock_moves");
    if (move) arm(move.row_id);
    form = { ...form, qty: "", cost: "", supplier: "", expiry: "" };
    await load();
  }

  async function undo() {
    if (!undoable || busy) return;
    const { moveId } = undoable;
    busy = true;
    status = null;
    const outcome = await submitIntent(
      { kind: "stock.in.reverted", move_id: moveId },
      newIdempotencyKey(),
    ).finally(() => (busy = false));
    stop();
    if (outcome.kind !== "applied") return void (await fail(outcome));
    status = { tone: "ok", message: "hub.stock.undone" };
    await load();
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.stock.title") })}</title>
</svelte:head>

<section class="stock" aria-labelledby="stock-title">
  <h1 class="type-hub-numeric" id="stock-title">{t("hub.nav.inventory")}</h1>
  <h2 class="type-hub-title">{t("hub.stock.title")}</h2>
  <p class="intro type-hub-body">{t("hub.stock.intro")}</p>
  <a class="link type-hub-label" href="/inventory">{t("hub.stock.back")}</a>

  <form class="form" onsubmit={save} data-testid="stock-form">
    <div class="field type-hub-label">
      <label for="stock-product">{t("hub.stock.product")}</label>
      <select
        id="stock-product"
        class="ui-interactive select"
        bind:value={form.product}
        aria-invalid={errors.product_id ? "true" : undefined}
        aria-describedby={errors.product_id ? "stock-product-error" : undefined}
      >
        <option value="">{t("hub.stock.product_pick")}</option>
        {#each tracked as p (p.id)}<option value={p.id}>{p.name}</option>{/each}
      </select>
      {#if errors.product_id}<p class="error" id="stock-product-error" role="alert">
          {t(errors.product_id)}
        </p>{/if}
    </div>
    <TextField label="hub.stock.qty" bind:value={form.qty} error={errors.qty} inputmode="numeric" />
    <TextField
      label="hub.stock.cost"
      bind:value={form.cost}
      error={errors.unit_cost_minor}
      inputmode="decimal"
    />
    <TextField label="hub.stock.supplier" bind:value={form.supplier} error={errors.supplier} />
    <div class="field type-hub-label">
      <label for="stock-expiry">{t("hub.stock.expiry")}</label>
      <input
        id="stock-expiry"
        class="ui-interactive select"
        type="date"
        bind:value={form.expiry}
        aria-invalid={errors.expiry ? "true" : undefined}
        aria-describedby={errors.expiry ? "stock-expiry-error" : undefined}
      />
      {#if errors.expiry}<p class="error" id="stock-expiry-error" role="alert">
          {t(errors.expiry)}
        </p>{/if}
    </div>
    <div class="actions">
      <Button label="hub.stock.submit" type="submit" />
    </div>
  </form>

  <div
    class="status type-hub-label"
    class:refusal={status?.tone === "refusal"}
    role="status"
    data-testid="stock-status"
  >
    {#if status}
      {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(status.message)}</span>
    {/if}
    {#if undoable}
      <button
        type="button"
        class="ui-interactive btn type-hub-label"
        data-testid="stock-undo"
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
    <ul class="rows" data-testid="stock-list">
      {#each tracked as p (p.id)}
        <li class="row" data-testid="stock-row" data-product-id={p.id}>
          <span class="type-hub-title">{p.name}</span>
          <span class="type-hub-label"
            >{t("hub.stock.row", { qty: p.quantity, cost: money(p.avg_cost_minor) })}</span
          >
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
