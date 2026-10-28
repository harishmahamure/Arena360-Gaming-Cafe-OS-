<script lang="ts">
  import { t } from "@arena/ui";
  import { page } from "$app/state";
  import { hasPermission } from "../../lib/auth";
  import { sessionStaff } from "../../lib/operating";
  import { REPORTS } from "../../lib/reports";

  const allowed = $derived(hasPermission(sessionStaff(page.data), "report.view"));
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.nav.reports") })}</title>
</svelte:head>

<section class="reports" data-testid="reports-screen">
  <h1 class="type-hub-numeric">{t("hub.nav.reports")}</h1>

  {#if allowed}
    <ul class="list" aria-label={t("hub.reports.list")}>
      {#each REPORTS as report (report.id)}
        <li>
          <a class="ui-interactive entry" href={report.href} data-testid="report-link-{report.id}">
            <span class="type-hub-body title">{t(report.title)}</span>
            <span class="type-hub-label note">{t(report.note)}</span>
          </a>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="type-hub-body note" role="status" data-testid="reports-denied">
      {t("hub.reports.no_access")}
    </p>
  {/if}
</section>

<style>
  .reports {
    display: grid;
    gap: var(--space-3);
    max-width: 40rem;
  }

  h1,
  p {
    margin: 0;
  }

  .list {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .entry {
    display: grid;
    gap: var(--space-1);
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    padding: var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
    text-decoration: none;
  }

  .title {
    font-weight: 600;
  }

  .note {
    color: var(--text-muted);
  }
</style>
