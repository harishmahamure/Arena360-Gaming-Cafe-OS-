<script lang="ts">
  import { StatusBadge, formatDateTime, t } from "@arena/ui";
  import type { GstArchiveEntry } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY } from "../../../lib/auth";
  import { loadGstArchives } from "../../../lib/gst-archive";
  import { branchLocale } from "../../../lib/locale.svelte";

  // System -> Backup: the yearly GST archives the Hub has saved to the cloud. The list is read
  // from the Hub's own log, so it is the same whether or not the cloud can be reached now.

  let archives: GstArchiveEntry[] | null = $state(null);
  let problem: "unreachable" | "denied" | null = $state(null);

  const tz = $derived(branchLocale.current?.tz);

  onMount(async () => {
    const outcome = await loadGstArchives();
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "ok") archives = outcome.archives;
    else problem = outcome.kind;
  });
</script>

<section class="backup" aria-labelledby="backup-title">
  <h2 class="type-hub-title" id="backup-title">{t("hub.backup.section_title")}</h2>

  <div class="gst" data-testid="gst-archives">
    <h3 class="type-hub-label">{t("hub.backup.gst.title")}</h3>
    <p class="type-hub-body">{t("hub.backup.gst.intro")}</p>
    {#if problem === "denied"}
      <p class="type-hub-body" role="status" data-testid="gst-denied">
        {t("hub.backup.gst.denied")}
      </p>
    {:else if problem}
      <p class="type-hub-body" role="status" data-testid="gst-unreadable">
        {t("hub.backup.gst.unreadable")}
      </p>
    {:else if archives && archives.length === 0}
      <p class="type-hub-body" data-testid="gst-none">{t("hub.backup.gst.none")}</p>
    {:else if archives}
      <ul class="years">
        {#each archives as a (a.fy)}
          <!-- i18n-ignore -->
          <li class="year" data-testid={`gst-archive-${a.fy}`}>
            <StatusBadge tone="ok" text={t("hub.backup.gst.saved", { year: a.label })} />
            <span class="type-hub-body">{t("hub.backup.gst.invoices", { count: a.invoices })}</span>
            <span class="type-hub-body"
              >{t("hub.backup.gst.credit_notes", { count: a.credit_notes })}</span
            >
            {#if a.orphaned > 0}
              <span class="type-hub-body"
                >{t("hub.backup.gst.orphaned", { count: a.orphaned })}</span
              >
            {/if}
            <span class="type-hub-body tabular-nums"
              >{t("hub.backup.gst.built", { when: formatDateTime(a.built_at, tz) })}</span
            >
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</section>

<style>
  .backup {
    display: grid;
    justify-items: start;
    gap: var(--space-4);
    max-width: 60ch;
  }

  h2,
  h3,
  p {
    margin: 0;
  }

  .gst {
    display: grid;
    gap: var(--space-3);
  }

  .years {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .year {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
</style>
