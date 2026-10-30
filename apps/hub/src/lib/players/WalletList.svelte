<script lang="ts">
  import { formatDateTime, formatMoney, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { SnapshotLocale, WalletView } from "@arena/ui/generated";

  let { wallet, locale }: { wallet: WalletView; locale: SnapshotLocale } = $props();

  const REASON: Record<string, MessageKey> = {
    topup: "hub.wallet.reason.topup",
    bill: "hub.wallet.reason.bill",
    erase_refund: "hub.wallet.reason.erase_refund",
    erase_forfeit: "hub.wallet.reason.erase_forfeit",
  };
  const METHOD: Record<string, MessageKey> = {
    cash: "hub.wallet.method.cash",
    upi: "hub.wallet.method.upi",
    card: "hub.wallet.method.card",
    mixed: "hub.wallet.method.mixed",
  };

  const rows = $derived.by(() => {
    let running = wallet.balance_minor;
    return wallet.txns.map((txn) => {
      const after = running;
      running -= txn.delta_minor;
      return { txn, after };
    });
  });

  const sign = (minor: number) => (minor > 0 ? "+" : "");
</script>

{#if rows.length === 0}
  <p class="type-hub-body" data-testid="wallet-empty">{t("hub.wallet.list.empty")}</p>
{:else}
  <table class="type-hub-body" data-testid="wallet-list">
    <thead>
      <tr>
        <th scope="col">{t("hub.wallet.col.time")}</th>
        <th scope="col">{t("hub.wallet.col.amount")}</th>
        <th scope="col">{t("hub.wallet.col.reason")}</th>
        <th scope="col">{t("hub.wallet.col.method")}</th>
        <th scope="col">{t("hub.wallet.col.staff")}</th>
        <th scope="col">{t("hub.wallet.col.balance")}</th>
      </tr>
    </thead>
    <tbody>
      {#each rows as { txn, after } (txn.id)}
        <tr data-testid="wallet-row">
          <td>{formatDateTime(txn.at, locale.tz)}</td>
          <td class="tabular-nums" data-testid="wallet-delta"
            >{sign(txn.delta_minor)}{formatMoney(txn.delta_minor, locale)}</td
          >
          <td>{t(REASON[txn.reason] ?? "hub.wallet.reason.bill")}</td>
          <td
            >{t(
              txn.method
                ? (METHOD[txn.method] ?? "hub.wallet.method.none")
                : "hub.wallet.method.none",
            )}</td
          >
          <td>{txn.staff}</td>
          <td class="tabular-nums" data-testid="wallet-running">{formatMoney(after, locale)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}

<style>
  p {
    margin: 0;
  }

  table {
    border-collapse: collapse;
    width: 100%;
  }

  th,
  td {
    padding: var(--space-2) var(--space-3);
    text-align: start;
    border-bottom: 1px solid var(--border-strong);
  }

  th {
    color: var(--text-muted);
    font-weight: var(--type-hub-label-weight);
  }
</style>
