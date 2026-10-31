<script lang="ts">
  import { Button, formatDateTime, t } from "@arena/ui";
  import type { InvoiceList, InvoiceRow } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import { signInUrl } from "./api";
  import { shell } from "./shell.svelte";
  import { loadInvoices, money, pdfLink } from "./subscription";

  /**
   * Every invoice and credit note across the café's branches, newest first, filterable by branch
   * and financial year, each with a short-lived signed link to its PDF. `announce` hands a sentence
   * to the screen's polite live region.
   */
  let { announce }: { announce: (text: string) => void } = $props();

  const id = $props.id();
  let branch = $state("");
  let fy = $state("");
  let list: InvoiceList | null = $state(null);
  let failed = $state(false);
  let opening: string | null = $state(null);
  let openFailed: string | null = $state(null);
  // Only the newest request may fill the list, whatever order the answers arrive in.
  let latest = 0;

  const branches = $derived(shell.branches ?? []);
  const zone = (branchId: string) => branches.find((b) => b.id === branchId)?.timezone;

  async function fetchList() {
    const mine = ++latest;
    const res = await loadInvoices(branch, fy);
    if (mine !== latest) return;
    if (res === "signed-out") return location.assign(signInUrl());
    failed = res === null;
    if (res) {
      list = res.data;
      announce(t("owner.invoices.count", { n: res.data.invoices.length }));
    }
  }

  async function open(row: InvoiceRow) {
    opening = row.id;
    openFailed = null;
    const link = await pdfLink(row);
    opening = null;
    if (link === "signed-out") return location.assign(signInUrl());
    if (link === "failed") openFailed = row.id;
    else location.assign(link.url);
  }

  onMount(() => void fetchList());
</script>

<section aria-labelledby="{id}-title" class="invoices" data-testid="invoices">
  <h2 id="{id}-title" class="type-hub-title">{t("owner.invoices.title")}</h2>
  <div class="filters">
    <label for="{id}-branch">{t("owner.invoices.branch")}</label>
    <select id="{id}-branch" class="ui-interactive select" bind:value={branch} onchange={fetchList}>
      <option value="">{t("owner.invoices.branch.all")}</option>
      {#each branches as b (b.id)}<option value={b.id}>{b.name}</option>{/each}
    </select>
    <label for="{id}-fy">{t("owner.invoices.year")}</label>
    <select id="{id}-fy" class="ui-interactive select" bind:value={fy} onchange={fetchList}>
      <option value="">{t("owner.invoices.year.all")}</option>
      {#each list?.years ?? [] as year (year)}<option value={year}>{year}</option>{/each}
    </select>
  </div>
  {#if failed && !list}
    <p>{t("owner.invoices.unavailable")}</p>
    <Button label="common.retry" variant="secondary" onclick={fetchList} />
  {:else if list && list.invoices.length === 0}
    <p>{t("owner.invoices.empty")}</p>
  {:else if list}
    <ul class="rows">
      {#each list.invoices as row (row.id)}
        <li class="row" data-testid="invoice-{row.number}">
          <div class="what">
            <strong class="tabular-nums">
              {row.kind === "credit_note"
                ? t("owner.invoices.credit_note", { number: row.number })
                : row.number}
            </strong>
            {#if row.original_number}
              <span class="muted"
                >{t("owner.invoices.against", { number: row.original_number })}</span
              >
            {/if}
            <span class="muted">
              {formatDateTime(row.issued_at, zone(row.branch_id), { dateStyle: "medium" })} · {row.branch_name}
            </span>
          </div>
          <span class="total tabular-nums">{money(row.total_minor, row.currency)}</span>
          {#if row.pdf_ready}
            <button
              type="button"
              class="ui-interactive download"
              aria-label={t("owner.invoices.download_for", { number: row.number })}
              disabled={opening === row.id}
              onclick={() => open(row)}>{t("owner.invoices.download")}</button
            >
          {:else}
            <span class="muted">{t("owner.invoices.preparing")}</span>
          {/if}
          {#if openFailed === row.id}
            <p class="failed" role="alert">{t("owner.invoices.failed")}</p>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .invoices {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin-top: var(--space-6);
  }
  h2,
  p {
    margin: 0;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
  }
  .select {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    background-color: var(--text-field-background);
    color: var(--text-field-foreground);
    font: inherit;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2) var(--space-4);
    padding: var(--space-3);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
  }
  .what {
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--text-muted);
  }
  .download {
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }
  .failed {
    flex-basis: 100%;
    color: var(--danger);
  }
</style>
