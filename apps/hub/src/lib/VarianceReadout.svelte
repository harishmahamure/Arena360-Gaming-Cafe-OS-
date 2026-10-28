<script lang="ts">
  import { formatMoney, t } from "@arena/ui";
  import type { ShiftCount, SnapshotLocale } from "@arena/ui/generated";

  /**
   * The variance readout of a closing shift (Story 5.26). Blind until a count is confirmed: it then
   * shows Counted, Expected and Difference as three figures and the staff-voice line, with the
   * breakdown of the expected cash. A difference over the branch threshold gets a warning border
   * and the label "Over threshold" (never red).
   */
  let {
    reveal,
    locale,
  }: {
    reveal: ShiftCount | null;
    locale: SnapshotLocale;
  } = $props();

  const partId = (id: string) => `breakdown-${id}`; // i18n-ignore (a test id)
  const money = (minor: number) => formatMoney(minor, locale);
  const difference = $derived(reveal?.difference_minor ?? 0);
  const line = $derived.by(() => {
    if (!reveal) return "";
    const args = { counted: money(reveal.counted_minor), expected: money(reveal.expected_minor) };
    if (difference < 0)
      return t("staff.shift.variance_short", { ...args, difference: money(difference) });
    if (difference > 0) {
      return t("staff.shift.variance_over", { ...args, difference: `+${money(difference)}` }); // i18n-ignore (a sign)
    }
    return t("staff.shift.variance_even", args);
  });
  const parts = $derived(
    reveal
      ? ([
          ["float", "hub.shift.close.breakdown.float", reveal.breakdown.float_minor],
          ["sales", "hub.shift.close.breakdown.sales", reveal.breakdown.cash_sales_minor],
          ["refunds", "hub.shift.close.breakdown.refunds", reveal.breakdown.cash_refunds_minor],
          ["deposits", "hub.shift.close.breakdown.deposits", reveal.breakdown.deposits_minor],
          ["expenses", "hub.shift.close.breakdown.expenses", reveal.breakdown.expenses_minor],
        ] as const)
      : [],
  );
</script>

<section
  class="readout"
  class:over={reveal?.over_threshold}
  aria-live="polite"
  data-testid="variance-readout"
>
  {#if !reveal}
    <p class="type-hub-body muted" data-testid="variance-blind">{t("hub.shift.close.blind")}</p>
  {:else}
    {#if reveal.over_threshold}
      <p class="type-hub-label warning" data-testid="variance-over">
        <span aria-hidden="true">⚠</span>
        {t("hub.shift.close.over_threshold")}
      </p>
    {/if}
    <div class="figures">
      <div>
        <span class="type-hub-label">{t("hub.shift.close.counted")}</span>
        <span class="type-hub-numeric tabular-nums" data-testid="variance-counted">
          {money(reveal.counted_minor)}
        </span>
      </div>
      <div>
        <span class="type-hub-label">{t("hub.shift.close.expected")}</span>
        <span class="type-hub-numeric tabular-nums" data-testid="variance-expected">
          {money(reveal.expected_minor)}
        </span>
      </div>
      <div>
        <span class="type-hub-label">{t("hub.shift.close.difference")}</span>
        <span class="type-hub-numeric tabular-nums" data-testid="variance-difference">
          {money(difference)}
        </span>
      </div>
    </div>
    <p class="type-hub-body" data-testid="variance-line">{line}</p>
    <details class="breakdown">
      <summary class="type-hub-label">{t("hub.shift.close.breakdown")}</summary>
      <dl class="type-hub-body" data-testid="variance-breakdown">
        {#each parts as [id, label, minor] (id)}
          <div>
            <dt>{t(label)}</dt>
            <dd class="tabular-nums" data-testid={partId(id)}>{money(minor)}</dd>
          </div>
        {/each}
      </dl>
    </details>
  {/if}
</section>

<style>
  .readout {
    display: grid;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
  }

  .readout.over {
    border: 2px solid var(--warning);
  }

  p,
  dl,
  dd {
    margin: 0;
  }

  .muted {
    color: var(--text-muted);
  }

  .warning {
    color: var(--warning-text, var(--text));
  }

  .figures {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-6);
  }

  .figures > div {
    display: grid;
    gap: var(--space-1);
  }

  dl {
    display: grid;
    gap: var(--space-2);
    margin-block-start: var(--space-2);
  }

  dl > div {
    display: flex;
    justify-content: space-between;
    gap: var(--space-3);
  }
</style>
