<script lang="ts">
  import { Button, StatusBadge, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { cmd } from "./commands.svelte";
  import PlanForm from "./PlanForm.svelte";
  import {
    fieldsOf,
    listed,
    moneyText,
    ratePerHour,
    rateEditable,
    rateLines,
    sourceOf,
    termsText,
    type BranchData,
    type Listed,
    type RateLine,
  } from "./pricing";
  import PricingRow from "./PricingRow.svelte";
  import RateForm from "./RateForm.svelte";

  let { data, scope }: { data: BranchData[]; scope: string } = $props();

  type Mode = { kind: "rate"; line: RateLine } | { kind: "plan"; item: Listed | null } | null;
  let mode: Mode = $state(null);

  const lines = $derived(rateLines(data));
  const plans = $derived(listed(data, (c) => c.plans));

  // The form closes once its change was sent or queued.
  let seen = cmd.sent;
  $effect(() => {
    if (cmd.sent !== seen) {
      seen = cmd.sent;
      mode = null;
    }
  });

  const KIND: Record<string, MessageKey> = {
    pack: "hub.plans.kind.pack",
    block: "hub.plans.kind.block",
    pass: "hub.plans.kind.pass",
  };
  const kindOf = (p: Listed) => {
    const k = fieldsOf(sourceOf(p)?.row ?? p.entries[0].row).plan_kind;
    return typeof k === "string" ? KIND[k] : undefined;
  };
  const groupsOf = (p: Listed) => {
    const e = sourceOf(p);
    const ids = fieldsOf(e?.row ?? p.entries[0].row).group_ids;
    return (Array.isArray(ids) ? ids : []).flatMap((id) => {
      const g = e?.data.cat?.groups.find((x) => x.id === id);
      return g ? [g.name] : [];
    });
  };
  const priceOf = (e: Listed["entries"][number]) => {
    const minor = fieldsOf(e.row).price_minor;
    return typeof minor === "number" ? moneyText(minor, e.data.currency) : "";
  };
</script>

<section class="panel" aria-labelledby="pr-rates">
  <h2 id="pr-rates" class="type-hub-title">{t("owner.pricing.rates.title")}</h2>
  {#if !lines.length}
    <p>{t("owner.pricing.rates.empty")}</p>
  {:else}
    <ul class="rows">
      {#each lines as line (line.key)}
        <PricingRow
          kind="rate"
          name={line.name}
          edit={rateEditable(line)
            ? {
                label: t("hub.groups.rate_edit", { name: line.name }),
                onclick: () => (mode = { kind: "rate", line }),
              }
            : undefined}
        >
          <span class="type-hub-label">{line.name}</span>
          <ul>
            {#each line.entries as e (e.data.branch.id)}
              {@const minor = e.rate ? ratePerHour(e.rate) : null}
              <li class="muted type-hub-micro">
                {t("owner.pricing.at", {
                  branch: e.data.branch.name,
                  text:
                    minor === null
                      ? t("owner.pricing.rate.none")
                      : t("hub.groups.rate_value", { rate: moneyText(minor, e.data.currency) }),
                })}
              </li>
            {/each}
          </ul>
        </PricingRow>
      {/each}
    </ul>
  {/if}
  {#if mode?.kind === "rate"}
    {#key mode.line.key}
      <RateForm line={mode.line} {data} {scope} oncancel={() => (mode = null)} />
    {/key}
  {/if}
</section>

<section class="panel" aria-labelledby="pr-plans">
  <h2 id="pr-plans" class="type-hub-title">{t("owner.pricing.plans.title")}</h2>
  {#if !plans.length}
    <p>{t("owner.pricing.plans.empty")}</p>
  {:else}
    <ul class="rows">
      {#each plans as p (p.key)}
        {@const kind = kindOf(p)}
        {@const source = sourceOf(p)}
        <PricingRow
          kind="plan"
          name={p.name}
          edit={source
            ? {
                label: t("owner.pricing.edit", { name: p.name }),
                onclick: () => (mode = { kind: "plan", item: p }),
              }
            : undefined}
        >
          <span class="type-hub-label">{p.name}</span>
          {#if kind}<span class="muted type-hub-micro">{t(kind)}</span>{/if}
          {#if source}
            <span class="muted type-hub-micro">{termsText(source.row)}</span>
            {#if groupsOf(p).length}
              <span class="muted type-hub-micro"
                >{t("hub.plans.for_groups", { names: groupsOf(p).join(", ") })}</span
              >
            {/if}
          {/if}
          <ul>
            {#each p.entries as e (e.data.branch.id)}
              <li class="muted type-hub-micro">
                {#if priceOf(e)}
                  {t("owner.pricing.at", { branch: e.data.branch.name, text: priceOf(e) })}
                {:else}
                  {e.data.branch.name}
                {/if}
                {#if e.row.active === false}
                  <StatusBadge tone="muted" text={t("owner.pricing.inactive")} />
                {/if}
              </li>
            {/each}
          </ul>
        </PricingRow>
      {/each}
    </ul>
  {/if}
  {#if mode?.kind === "plan"}
    {#key mode.item?.key ?? ""}
      <PlanForm item={mode.item} {data} {scope} oncancel={() => (mode = null)} />
    {/key}
  {:else}
    <div>
      <Button
        label="hub.plans.add"
        variant="secondary"
        onclick={() => (mode = { kind: "plan", item: null })}
      />
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
</style>
