<script lang="ts">
  import { Button, InlinePin, t } from "@arena/ui";
  import type { LowStockItem, LowStockView } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, untrack } from "svelte";
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
  import { loadLowStock, parseThreshold } from "../../../lib/lowstock";
  import { sessionStaff } from "../../../lib/operating";

  type Job = { item: LowStockItem; value: number | null; key: string };

  let view: LowStockView | null = $state(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let drafts: Record<string, string> = $state({});
  let errors: Record<string, MessageKey> = $state({});
  let busy = $state(false);
  let approving: Job | null = $state(null);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let loads = 0;

  const staff = $derived(sessionStaff(page.data));
  const needsApproval = $derived(!hasPermission(staff, "catalog.edit"));

  const shown = (item: LowStockItem) =>
    item.low_stock_qty === null ? "" : String(item.low_stock_qty);
  const draftOf = (item: LowStockItem) => drafts[item.product_id] ?? shown(item);

  async function load() {
    const mine = ++loads;
    const list = await loadLowStock();
    if (mine !== loads) return;
    loading = false;
    if (list.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    loadFailed = list.kind !== "ok";
    if (list.kind === "ok") view = list.view;
  }

  onMount(() => void load());
  $effect(() => {
    if (catalog.version) untrack(() => void load());
  });

  async function run(job: Job, ticket?: string) {
    if (needsApproval && !ticket) {
      approvalPrompt = "ui.pin.prompt";
      approving = job;
      return;
    }
    if (busy) {
      if (ticket) void dropElevation(ticket);
      return;
    }
    busy = true;
    status = null;
    const outcome = await submitIntent(
      { kind: "stock.threshold", product_id: job.item.product_id, low_stock_qty: job.value },
      job.key,
      undefined,
      { elevationTicket: ticket },
    ).finally(() => (busy = false));
    if (ticket) void dropElevation(ticket);
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "error" && outcome.error.code === "PIN_REQUIRED") {
      approvalPrompt = "ui.pin.again";
      approving = job;
      return;
    }
    if (outcome.kind === "applied") {
      delete drafts[job.item.product_id];
      status = { tone: "ok", message: "hub.lowstock.saved" };
    } else {
      const code = outcome.kind === "error" ? outcome.error.code : null;
      status = {
        tone: "refusal",
        message:
          code === "STALE"
            ? "hub.lowstock.stale"
            : code === "VALIDATION_FAILED"
              ? "hub.lowstock.refused"
              : code === "PERMISSION_DENIED"
                ? "hub.refusal.permission"
                : "hub.settings.failed",
      };
    }
    await load();
  }

  async function approve(pin: string) {
    const job = approving;
    if (!job) return { ok: false, message: "ui.pin.failed" } as const;
    const outcome = await elevate([{ intent_kind: "stock.threshold" }], pin);
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

  function save(event: SubmitEvent, item: LowStockItem) {
    event.preventDefault();
    if (busy || approving) return;
    const value = parseThreshold(draftOf(item));
    status = null;
    if (value === undefined) {
      errors[item.product_id] = "hub.lowstock.invalid";
      return;
    }
    delete errors[item.product_id];
    void run({ item, value, key: newIdempotencyKey() });
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.lowstock.title") })}</title>
</svelte:head>

<section class="stock" aria-labelledby="low-stock-title">
  <h1 class="type-hub-numeric" id="low-stock-title">{t("hub.nav.inventory")}</h1>
  <h2 class="type-hub-title">{t("hub.lowstock.title")}</h2>
  <p class="intro type-hub-body">{t("hub.lowstock.intro")}</p>
  <a class="link type-hub-label" href="/inventory">{t("hub.lowstock.back")}</a>

  {#if approving}
    <div class="approval" data-testid="low-stock-approval">
      <p class="type-hub-label">{t("hub.lowstock.approves")}</p>
      <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = null)} />
    </div>
  {/if}

  <div
    class="status type-hub-label"
    class:refusal={status?.tone === "refusal"}
    role="status"
    data-testid="low-stock-status"
  >
    {#if status}
      {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(status.message)}</span>
    {/if}
  </div>

  {#if loadFailed}
    <p class="type-hub-body" role="alert">{t("hub.lowstock.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void load()} />
  {:else if loading || !view}
    <p class="type-hub-body" aria-busy="true">…</p>
  {:else}
    <h3 class="type-hub-title">{t("hub.lowstock.low_heading")}</h3>
    {#if view.low.length === 0}
      <p class="type-hub-body">{t("hub.lowstock.empty")}</p>
    {:else}
      <ul class="rows" data-testid="low-stock-list">
        {#each view.low as item (item.product_id)}
          <li class="row" data-testid="low-stock-row" data-product-id={item.product_id}>
            <span class="type-hub-title">{item.product}</span>
            <span class="type-hub-label"
              >{t("hub.lowstock.row", { qty: item.qty, threshold: item.low_stock_qty ?? 0 })}</span
            >
          </li>
        {/each}
      </ul>
    {/if}

    <h3 class="type-hub-title">{t("hub.lowstock.thresholds_heading")}</h3>
    <p class="intro type-hub-label">{t("hub.lowstock.help")}</p>
    {#if view.tracked.length === 0}
      <p class="type-hub-body">{t("hub.lowstock.thresholds_empty")}</p>
    {:else}
      <ul class="rows" data-testid="threshold-list">
        {#each view.tracked as item (item.product_id)}
          <li class="row" data-testid="threshold-row" data-product-id={item.product_id}>
            <form class="edit" onsubmit={(e) => save(e, item)}>
              <span class="type-hub-title name">{item.product}</span>
              <span class="type-hub-label">{t("hub.lowstock.on_hand", { qty: item.qty })}</span>
              <input
                class="ui-interactive field"
                inputmode="numeric"
                autocomplete="off"
                aria-label={t("hub.lowstock.threshold_for", { product: item.product })}
                aria-invalid={errors[item.product_id] ? "true" : undefined}
                value={draftOf(item)}
                oninput={(e) => (drafts[item.product_id] = e.currentTarget.value)}
              />
              <button
                type="submit"
                class="ui-interactive btn type-hub-label"
                aria-label={t("hub.lowstock.save_for", { product: item.product })}
                disabled={busy}>{t("hub.lowstock.save")}</button
              >
              {#if errors[item.product_id]}
                <span class="error type-hub-label" role="alert"
                  >{t(errors[item.product_id] as MessageKey)}</span
                >
              {/if}
            </form>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<style>
  .stock {
    display: grid;
    gap: var(--space-3);
    max-width: 48rem;
  }

  h1,
  h2,
  h3,
  p {
    margin: 0;
  }

  .intro {
    color: var(--text-muted);
  }

  .link {
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

  .edit {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    inline-size: 100%;
  }

  .name {
    flex: 1 1 10rem;
  }

  .field {
    inline-size: 7rem;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
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

  .error,
  .status.refusal {
    color: var(--text-field-error-text);
  }

  .status {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    min-height: 1lh;
  }
</style>
