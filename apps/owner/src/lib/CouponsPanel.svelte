<script lang="ts">
  import { Button, StatusBadge, t } from "@arena/ui";
  import { cmd } from "./commands.svelte";
  import CouponForm from "./CouponForm.svelte";
  import { listed, offText, sourceOf, windowText, type BranchData, type Listed } from "./pricing";
  import PricingRow from "./PricingRow.svelte";

  let { data, scope }: { data: BranchData[]; scope: string } = $props();

  type Mode = { item: Listed | null } | null;
  let mode: Mode = $state(null);

  const coupons = $derived(listed(data, (c) => c.coupons));

  // The form closes once its change was sent or queued.
  let seen = cmd.sent;
  $effect(() => {
    if (cmd.sent !== seen) {
      seen = cmd.sent;
      mode = null;
    }
  });
</script>

<section class="panel" aria-labelledby="pr-coupons">
  <h2 id="pr-coupons" class="type-hub-title">{t("owner.pricing.tab.coupons")}</h2>
  {#if !coupons.length}
    <p>{t("owner.pricing.coupons.empty")}</p>
  {:else}
    <ul class="rows">
      {#each coupons as c (c.key)}
        {@const source = sourceOf(c)}
        <PricingRow
          kind="coupon"
          name={c.name}
          edit={source
            ? {
                label: t("hub.coupons.edit", { code: c.name }),
                onclick: () => (mode = { item: c }),
              }
            : undefined}
        >
          <span class="type-hub-label">{c.name}</span>
          {#if source}
            <span class="muted type-hub-micro"
              >{offText(source.row, source.data.currency)} · {windowText(source.row)}</span
            >
          {/if}
          <span class="muted type-hub-micro"
            >{t("owner.pricing.branches", {
              names: c.entries.map((e) => e.data.branch.name).join(", "),
            })}</span
          >
          {#if c.entries.some((e) => e.row.active === false)}
            <span><StatusBadge tone="muted" text={t("hub.coupons.badge.inactive")} /></span>
          {/if}
        </PricingRow>
      {/each}
    </ul>
  {/if}
  {#if mode}
    {#key mode.item?.key ?? ""}
      <CouponForm item={mode.item} {data} {scope} oncancel={() => (mode = null)} />
    {/key}
  {:else}
    <div>
      <Button label="hub.coupons.add" variant="secondary" onclick={() => (mode = { item: null })} />
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
