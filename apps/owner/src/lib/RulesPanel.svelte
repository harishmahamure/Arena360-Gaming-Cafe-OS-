<script lang="ts">
  import { Button, StatusBadge, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { cmd } from "./commands.svelte";
  import {
    adjustText,
    fieldsOf,
    listed,
    ruleStatus,
    scheduleText,
    sourceOf,
    type BranchData,
    type Listed,
    type RuleStatus,
  } from "./pricing";
  import PricingRow from "./PricingRow.svelte";
  import RuleForm from "./RuleForm.svelte";

  let { data, scope, now }: { data: BranchData[]; scope: string; now: number } = $props();

  type Mode = { item: Listed | null } | null;
  let mode: Mode = $state(null);

  const rules = $derived(listed(data, (c) => c.pricing_rules));

  // The form closes once its change was sent or queued.
  let seen = cmd.sent;
  $effect(() => {
    if (cmd.sent !== seen) {
      seen = cmd.sent;
      mode = null;
    }
  });

  const STATUS: Record<RuleStatus, { key: MessageKey; tone: "ok" | "info" | "muted" }> = {
    active: { key: "owner.pricing.rule.status.active", tone: "ok" },
    scheduled: { key: "owner.pricing.rule.status.scheduled", tone: "info" },
    ended: { key: "owner.pricing.rule.status.ended", tone: "muted" },
    off: { key: "owner.pricing.rule.status.off", tone: "muted" },
  };
  const groupOf = (e: Listed["entries"][number]) =>
    e.data.cat?.groups.find((g) => g.id === fieldsOf(e.row).group_id)?.name;
</script>

<section class="panel" aria-labelledby="pr-rules">
  <h2 id="pr-rules" class="type-hub-title">{t("owner.pricing.tab.rules")}</h2>
  {#if !rules.length}
    <p>{t("owner.pricing.rules.empty")}</p>
  {:else}
    <ul class="rows">
      {#each rules as r (r.key)}
        {@const source = sourceOf(r)}
        <PricingRow
          kind="rule"
          name={r.name}
          edit={source
            ? {
                label: t("owner.pricing.edit", { name: r.name }),
                onclick: () => (mode = { item: r }),
              }
            : undefined}
        >
          <span class="type-hub-label">{r.name}</span>
          {#if source}
            <span class="muted type-hub-micro">{scheduleText(source.row)}</span>
            <span class="muted type-hub-micro"
              >{[groupOf(source), adjustText(source.row, source.data.currency)]
                .filter(Boolean)
                .join(" · ")}</span
            >
          {/if}
          <span class="muted type-hub-micro"
            >{t("owner.pricing.branches", {
              names: r.entries.map((e) => e.data.branch.name).join(", "),
            })}</span
          >
          <ul>
            {#each r.entries as e (e.data.branch.id)}
              {@const status = ruleStatus(e.row, e.data.branch.timezone, now)}
              <li class="status">
                <span class="muted type-hub-micro">{e.data.branch.name}</span>
                {#if status}
                  <StatusBadge tone={STATUS[status].tone} text={t(STATUS[status].key)} />
                {/if}
              </li>
            {/each}
          </ul>
        </PricingRow>
      {/each}
    </ul>
  {/if}
  {#if mode}
    {#key mode.item?.key ?? ""}
      <RuleForm item={mode.item} {data} {scope} oncancel={() => (mode = null)} />
    {/key}
  {:else}
    <div>
      <Button label="hub.rules.add" variant="secondary" onclick={() => (mode = { item: null })} />
    </div>
  {/if}
</section>

<style>
  .panel {
    display: grid;
    gap: var(--space-3);
  }
  h2,
  p {
    margin: 0;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--line);
  }
  .muted {
    color: var(--text-muted);
  }
  .status {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
</style>
