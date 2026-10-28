<script lang="ts">
  import { TextField, formatMoney, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { SnapshotLocale } from "@arena/ui/generated";
  import { parseAmountOrZero, parseCount, parseFloat } from "./shift";

  /**
   * The cash count of a closing shift (Story 5.26): one row per note of the branch currency with a
   * count field and a subtotal, a coins total and a running total, or a single total. A currency
   * with no note set is counted as a single total only. `total` is the count in minor units, or
   * `null` while a field is not a valid number. The expected cash is never shown here.
   */
  let {
    denominations,
    locale,
    total = $bindable(null as number | null),
    error = null,
  }: {
    denominations: readonly number[];
    locale: SnapshotLocale;
    total?: number | null;
    error?: MessageKey | null;
  } = $props();

  const fieldId = (d: number) => `count-${d}`; // i18n-ignore (an element id)
  const hasNotes = $derived(denominations.length > 0);
  let single = $state(false);
  let singleText = $state("");
  let coinsText = $state("");
  let counts = $state({} as Record<number, string>);

  const useSingle = $derived(single || !hasNotes);
  const rows = $derived(
    denominations.map((d) => {
      const n = parseCount(counts[d] ?? "");
      return { d, n, subtotal: n === null ? null : n * d };
    }),
  );
  const sum = $derived.by(() => {
    if (useSingle) return parseFloat(singleText, locale.exponent);
    const coins = parseAmountOrZero(coinsText, locale.exponent);
    if (coins === null || rows.some((r) => r.subtotal === null)) return null;
    return rows.reduce((acc, r) => acc + (r.subtotal ?? 0), coins);
  });

  $effect(() => {
    total = sum;
  });
</script>

<div class="counter" data-testid="denomination-counter">
  {#if hasNotes}
    <label class="toggle type-hub-body">
      <input type="checkbox" bind:checked={single} data-testid="count-single-toggle" />
      <span>{t("hub.shift.close.single_total")}</span>
    </label>
  {/if}
  {#if useSingle}
    <TextField
      label="hub.shift.close.counted_cash"
      bind:value={singleText}
      inputmode="decimal"
      autocomplete="off"
      {error}
    />
  {:else}
    <ul class="rows">
      {#each rows as r (r.d)}
        <li class="row">
          <label class="note type-hub-body" for={fieldId(r.d)}>{formatMoney(r.d, locale)}</label>
          <input
            id={fieldId(r.d)}
            class="ui-interactive count tabular-nums"
            inputmode="numeric"
            autocomplete="off"
            bind:value={counts[r.d]}
            aria-invalid={r.n === null ? "true" : undefined}
            data-testid={fieldId(r.d)}
          />
          <span class="subtotal type-hub-numeric tabular-nums">
            {r.subtotal === null ? "" : formatMoney(r.subtotal, locale)}
          </span>
        </li>
      {/each}
    </ul>
    <TextField
      label="hub.shift.close.coins"
      bind:value={coinsText}
      inputmode="decimal"
      autocomplete="off"
      {error}
    />
  {/if}
  <p class="running type-hub-body" data-testid="count-total">
    <span class="type-hub-label">{t("hub.shift.close.running_total")}</span>
    <span class="type-hub-numeric tabular-nums">
      {total === null ? "" : formatMoney(total, locale)}
    </span>
  </p>
</div>

<style>
  .counter {
    display: grid;
    gap: var(--space-3);
  }

  .rows {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: grid;
    grid-template-columns: 7rem 6rem 1fr;
    align-items: center;
    gap: var(--space-3);
  }

  .count {
    min-block-size: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface);
    color: var(--text);
    font: inherit;
    text-align: end;
  }

  .subtotal {
    text-align: end;
  }

  .toggle {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
    cursor: pointer;
  }

  .toggle input {
    flex: none;
    width: var(--space-5);
    height: var(--space-5);
    margin: 0;
    accent-color: var(--accent);
  }

  .running {
    display: flex;
    justify-content: space-between;
    margin: 0;
    padding-block-start: var(--space-2);
    border-block-start: 1px solid var(--border-strong);
  }
</style>
