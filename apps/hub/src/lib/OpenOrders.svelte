<script lang="ts">
  import { Button, formatMoney, t } from "@arena/ui";
  import type {
    BusinessBuyer,
    PaymentRow,
    ReceiptChoice,
    SessionStaff,
    SnapshotBill,
    SnapshotLocale,
  } from "@arena/ui/generated";
  import { hasPermission, newIdempotencyKey, submitIntent } from "./auth";
  import type { AppliedDiscount } from "./discount";
  import SettleBar from "./SettleBar.svelte";
  import { openCounterOrders } from "./open-orders";

  let {
    bills,
    names,
    locale,
    staff,
    ondone = () => {},
  }: {
    /** The snapshot's bills; the unpaid counter orders (no session) are the ones listed. */
    bills: SnapshotBill[];
    /** Device labels by device id. */
    names: Record<string, string>;
    locale: SnapshotLocale;
    staff: SessionStaff | null;
    /** An order was settled: the page reloads what it shows. */
    ondone?: () => void;
  } = $props();

  const orders = $derived(openCounterOrders(bills));
  let loaded: string | null | undefined = $state(null);
  let busy = $state(false);
  let error: string | null = $state(null);
  const bill = $derived(orders.find((b) => b.order_id === loaded) ?? null);

  const money = (minor: number) => formatMoney(minor, locale);

  async function settle(
    payments: PaymentRow[],
    receipt?: ReceiptChoice,
    business?: BusinessBuyer,
    coupon?: string,
    discount?: AppliedDiscount,
  ) {
    if (busy || bill === null || bill.order_id == null) return;
    busy = true;
    error = null;
    const outcome = await submitIntent(
      {
        kind: "session.settle",
        order_id: bill.order_id,
        payments,
        receipt,
        business,
        coupon,
        discount: discount?.discount,
      },
      newIdempotencyKey(),
      bill.order_rev ?? undefined,
      { elevationTicket: discount?.ticket },
    ).finally(() => (busy = false));
    if (outcome.kind === "applied") {
      loaded = null;
      ondone();
      return;
    }
    error = outcome.kind === "error" ? outcome.error.message : t("hub.pos.err.unreachable");
  }
</script>

<section class="open" data-testid="open-orders" aria-labelledby="open-orders-title">
  <h2 class="type-hub-title" id="open-orders-title">{t("hub.pos.open.title")}</h2>
  {#if orders.length === 0}
    <p class="type-hub-body" role="status" data-testid="open-orders-empty">
      {t("hub.pos.open.empty")}
    </p>
  {:else}
    <ul class="orders" aria-label={t("hub.pos.open.list")}>
      {#each orders as order (order.order_id)}
        <li class="order" data-testid="open-order" data-order-id={order.order_id}>
          <span class="type-hub-body" data-testid="open-order-pc">
            {t("hub.pos.open.from", { pc: names[order.device_id] ?? order.device_id })}
          </span>
          <span class="type-hub-label" data-testid="open-order-items">
            {(order.items ?? [])
              .map((i) => t("hub.pos.open.item", { qty: i.qty, label: i.label }))
              .join(", ")}
          </span>
          <span class="type-hub-numeric tabular-nums" data-testid="open-order-total">
            {money(order.remaining_minor)}
          </span>
          <Button
            label="hub.pos.open.load"
            variant="secondary"
            disabled={busy}
            disabledReason="hub.sheet.waiting"
            onclick={() => (loaded = loaded === order.order_id ? null : order.order_id)}
          />
        </li>
      {/each}
    </ul>
  {/if}

  {#if bill}
    <SettleBar
      {bill}
      {locale}
      {busy}
      onsettle={settle}
      title="hub.pos.open.settle"
      prefillCash
      receiptChoice
      couponSource={bill.order_id ? { order_id: bill.order_id } : null}
      discountDirect={hasPermission(staff, "discount.manual")}
    />
  {/if}
  {#if error}
    <p class="refusal type-hub-label" role="alert" data-testid="open-orders-error">
      <span aria-hidden="true">⚠</span>
      {error}
    </p>
  {/if}
</section>

<style>
  .open {
    display: grid;
    gap: var(--space-3);
  }

  .orders {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .order {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: var(--space-2) var(--space-3);
    align-items: center;
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }

  .order [data-testid="open-order-items"] {
    grid-column: 1 / -1;
    color: var(--text-muted);
  }

  .refusal {
    margin: 0;
  }
</style>
