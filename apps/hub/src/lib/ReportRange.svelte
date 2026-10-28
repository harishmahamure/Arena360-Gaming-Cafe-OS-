<script lang="ts">
  import { Button, t } from "@arena/ui";
  import { branchLocale } from "./locale.svelte";
  import { MAX_DAYS, addDays, businessDayNow, daySpan } from "./reports";

  /** The business-day range a report covers, in branch local time. Presets and the two date fields share it. */
  let {
    from = $bindable(""),
    to = $bindable(""),
    onchange,
  }: { from: string; to: string; onchange: () => void } = $props();

  const locale = $derived(branchLocale.current);
  const span = $derived(daySpan(from, to));
  const valid = $derived(span >= 1 && span <= MAX_DAYS);

  /** Picks the last `days` business days, ending with today. */
  function preset(days: number, endsYesterday = false) {
    if (!locale) return;
    const today = businessDayNow(locale, Date.now());
    const end = endsYesterday ? addDays(today, -1) : today;
    from = addDays(end, -(days - 1));
    to = end;
    onchange();
  }

  function apply(event: SubmitEvent) {
    event.preventDefault();
    if (valid) onchange();
  }
</script>

<form class="range" onsubmit={apply} data-testid="report-range">
  <label class="field type-hub-label">
    <span>{t("hub.reports.from")}</span>
    <input class="ui-interactive" type="date" bind:value={from} data-testid="report-from" />
  </label>
  <label class="field type-hub-label">
    <span>{t("hub.reports.to")}</span>
    <input class="ui-interactive" type="date" bind:value={to} data-testid="report-to" />
  </label>
  <Button label="hub.reports.show" type="submit" />
  <div class="presets" role="group" aria-label={t("hub.reports.presets")}>
    <Button label="hub.reports.today" variant="secondary" onclick={() => preset(1)} />
    <Button label="hub.reports.yesterday" variant="secondary" onclick={() => preset(1, true)} />
    <Button label="hub.reports.last_7" variant="secondary" onclick={() => preset(7)} />
    <Button label="hub.reports.last_30" variant="secondary" onclick={() => preset(30)} />
  </div>
  {#if from && to && !valid}
    <p class="type-hub-label problem" role="alert" data-testid="report-range-problem">
      {span < 1 ? t("hub.reports.range_order") : t("hub.reports.range_long")}
    </p>
  {/if}
</form>

<style>
  .range {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: var(--space-3);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  input {
    --ui-bg: var(--text-field-background);
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--text-field-foreground);
    font: inherit;
    font-size: var(--type-web-body-size);
  }

  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .problem {
    flex-basis: 100%;
    margin: 0;
    color: var(--text-field-error-text);
  }
</style>
