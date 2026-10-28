<script lang="ts">
  import { Button, InlinePin, t } from "@arena/ui";
  import type { ExpiringLot, ExpiryView } from "@arena/ui/generated";
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
  import { loadExpiry } from "../../../lib/expiry";
  import { branchLocale } from "../../../lib/locale.svelte";
  import { sessionStaff } from "../../../lib/operating";
  import { priceText } from "../../../lib/products";

  type Job = { lot: ExpiringLot; key: string };

  let view: ExpiryView | null = $state(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let busy = $state(false);
  let approving: Job | null = $state(null);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let loads = 0;

  const staff = $derived(sessionStaff(page.data));
  const needsApproval = $derived(!hasPermission(staff, "stock.adjust"));
  const exponent = $derived(branchLocale.current?.exponent ?? 2);
  const symbol = $derived(branchLocale.current?.symbol ?? "");

  const money = (minor: number) => {
    const parts = [symbol, priceText(minor, exponent)];
    return (branchLocale.current?.symbol_position === "after" ? parts.reverse() : parts).join("");
  };

  async function load() {
    const mine = ++loads;
    const list = await loadExpiry();
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
      { kind: "stock.write_off", lot_id: job.lot.lot_id },
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
      status = { tone: "ok", message: "hub.expired.done" };
    } else {
      const code = outcome.kind === "error" ? outcome.error.code : null;
      status = {
        tone: "refusal",
        message:
          code === "STALE"
            ? "hub.expired.stale"
            : code === "VALIDATION_FAILED"
              ? "hub.expired.invalid"
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
    const outcome = await elevate([{ intent_kind: "stock.write_off" }], pin);
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

  function writeOff(lot: ExpiringLot) {
    if (busy) return;
    void run({ lot, key: newIdempotencyKey() });
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.expired.title") })}</title>
</svelte:head>

<section class="stock" aria-labelledby="expired-title">
  <h1 class="type-hub-numeric" id="expired-title">{t("hub.nav.inventory")}</h1>
  <h2 class="type-hub-title">{t("hub.expired.title")}</h2>
  <p class="intro type-hub-body">{t("hub.expired.intro")}</p>
  <a class="link type-hub-label" href="/inventory">{t("hub.expiry.back")}</a>
  <a class="link type-hub-label" href="/inventory/expiring">{t("hub.expired.expiring_link")}</a>

  {#if approving}
    <div class="approval" data-testid="expired-approval">
      <p class="type-hub-label">{t("hub.expired.approves")}</p>
      <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = null)} />
    </div>
  {/if}

  <div
    class="status type-hub-label"
    class:refusal={status?.tone === "refusal"}
    role="status"
    data-testid="expired-status"
  >
    {#if status}
      {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(status.message)}</span>
    {/if}
  </div>

  {#if loadFailed}
    <p class="type-hub-body" role="alert">{t("hub.expiry.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void load()} />
  {:else if loading || !view}
    <p class="type-hub-body" aria-busy="true">…</p>
  {:else if view.expired.length === 0}
    <p class="type-hub-body">{t("hub.expired.empty")}</p>
  {:else}
    <ul class="rows" data-testid="expired-list">
      {#each view.expired as lot (lot.lot_id)}
        <li class="row" data-testid="expired-row" data-lot-id={lot.lot_id}>
          <span class="type-hub-title">{lot.product}</span>
          <span class="type-hub-label"
            >{t("hub.expired.row", { qty: lot.qty, date: lot.expiry })}</span
          >
          <span class="type-hub-label">{t("hub.expiry.col_value")}: {money(lot.value_minor)}</span>
          <button
            type="button"
            class="ui-interactive btn type-hub-label"
            data-testid="expired-write-off"
            disabled={busy}
            onclick={() => writeOff(lot)}>{t("hub.expired.write_off")}</button
          >
        </li>
      {/each}
    </ul>
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
