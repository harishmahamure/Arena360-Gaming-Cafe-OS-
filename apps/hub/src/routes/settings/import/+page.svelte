<script lang="ts">
  import { TabControl, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { page } from "$app/state";
  import { hasPermission } from "../../../lib/auth";
  import ImportPanel from "../../../lib/ImportPanel.svelte";
  import SetupPanel from "../../../lib/SetupPanel.svelte";
  import StarterPanel from "../../../lib/StarterPanel.svelte";
  import {
    commitPlans,
    commitPlayers,
    commitProducts,
    previewPlans,
    previewPlayers,
    previewProducts,
    savePlanTemplate,
    savePlayerTemplate,
    saveProductTemplate,
    type ImportKind,
  } from "../../../lib/import";
  import { sessionStaff } from "../../../lib/operating";

  const staff = $derived(sessionStaff(page.data));
  const canCatalog = $derived(hasPermission(staff, "catalog.edit"));
  const canPlayers = $derived(hasPermission(staff, "player.manage"));

  const TABS: { id: ImportKind; label: MessageKey }[] = [
    { id: "products", label: "hub.import.tab_products" },
    { id: "players", label: "hub.import.tab_players" },
    { id: "plans", label: "hub.import.tab_plans" },
    { id: "templates", label: "hub.import.tab_templates" },
    { id: "setup", label: "hub.import.tab_setup" },
  ];
  let kind: ImportKind = $state("products");
</script>

<section class="import" aria-labelledby="settings-import-title" data-testid="settings-import">
  <h2 class="type-hub-title" id="settings-import-title">{t("hub.import.title")}</h2>

  <TabControl
    items={TABS}
    label="hub.import.tabs"
    selected={kind}
    onchange={(id) => (kind = id as ImportKind)}
  >
    {#snippet panel(current)}
      {#key current}
        {#if current === "products"}
          <ImportPanel
            allowed={canCatalog}
            noAccess="hub.import.no_access"
            title="hub.import.products_title"
            intro="hub.import.products_intro"
            hint="hub.import.file_hint"
            check={previewProducts}
            apply={commitProducts}
            saveTemplate={saveProductTemplate}
            summary={(p) =>
              t("hub.import.summary", {
                rows: p.rows,
                new: p.new,
                updated: p.updated,
                failed: p.failed,
              })}
            note={(p) =>
              p.with_stock > 0 ? t("hub.import.stock_note", { count: p.with_stock }) : null}
            done={(r) => t("hub.import.done", { created: r.created, updated: r.updated })}
            skipped={(r) => r.skipped}
          />
        {:else if current === "players"}
          <ImportPanel
            allowed={canPlayers}
            noAccess="hub.import.players_no_access"
            title="hub.import.players_title"
            intro="hub.import.players_intro"
            hint="hub.import.players_hint"
            check={previewPlayers}
            apply={commitPlayers}
            saveTemplate={savePlayerTemplate}
            summary={(p) =>
              t("hub.import.players_summary", {
                rows: p.rows,
                new: p.new,
                matched: p.matched,
                failed: p.failed,
              })}
            note={(p) =>
              p.with_balance > 0 || p.with_credit > 0
                ? t("hub.import.players_note", { balance: p.with_balance, credit: p.with_credit })
                : null}
            done={(r) => t("hub.import.players_done", { created: r.created, matched: r.matched })}
            skipped={(r) => r.skipped}
          />
        {:else if current === "templates"}
          <StarterPanel allowed={canCatalog} />
        {:else if current === "setup"}
          <SetupPanel allowed={canCatalog} />
        {:else}
          <ImportPanel
            allowed={canCatalog}
            noAccess="hub.import.plans_no_access"
            title="hub.import.plans_title"
            intro="hub.import.plans_intro"
            hint="hub.import.plans_hint"
            check={previewPlans}
            apply={commitPlans}
            saveTemplate={savePlanTemplate}
            summary={(p) =>
              t("hub.import.plans_summary", {
                rows: p.rows,
                new: p.new,
                updated: p.updated,
                failed: p.failed,
              })}
            done={(r) => t("hub.import.plans_done", { created: r.created, updated: r.updated })}
            skipped={(r) => r.skipped}
          />
        {/if}
      {/key}
    {/snippet}
  </TabControl>
</section>

<style>
  .import {
    display: grid;
    gap: var(--space-3);
  }

  h2 {
    margin: 0;
  }
</style>
