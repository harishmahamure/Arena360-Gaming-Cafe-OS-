<script lang="ts">
  import { Button, StatusBadge, TextField, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { cmd } from "./commands.svelte";
  import { listed, sourceOf, fieldsOf, type BranchData, type Listed } from "./pricing";
  import PricingRow from "./PricingRow.svelte";
  import ProductForm from "./ProductForm.svelte";
  import {
    barcodeOf,
    bundlesOf,
    containsText,
    matches,
    priceLine,
    productsOf,
    thresholdOf,
    tracked,
  } from "./products";
  import ThresholdForm from "./ThresholdForm.svelte";

  let { data, scope }: { data: BranchData[]; scope: string } = $props();

  type Mode =
    | { kind: "product"; item: Listed | null; bundle: boolean }
    | { kind: "threshold"; item: Listed }
    | null;
  let mode: Mode = $state(null);
  let query = $state("");

  const products = $derived(listed(data, productsOf).filter((p) => matches(p, query)));
  const bundles = $derived(listed(data, bundlesOf).filter((b) => matches(b, query)));
  const sections = $derived<
    { id: string; title: MessageKey; empty: MessageKey; items: Listed[]; bundle: boolean }[]
  >([
    {
      id: "products",
      title: "owner.products.title",
      empty: "hub.products.empty",
      items: products,
      bundle: false,
    },
    {
      id: "bundles",
      title: "owner.products.bundles",
      empty: "owner.products.bundles_empty",
      items: bundles,
      bundle: true,
    },
  ]);

  // The form closes once its change was sent or queued.
  let seen = cmd.sent;
  $effect(() => {
    if (cmd.sent !== seen) {
      seen = cmd.sent;
      mode = null;
    }
  });

  const categoryOf = (p: Listed) => {
    const c = fieldsOf(sourceOf(p)?.row ?? p.entries[0].row).category;
    return typeof c === "string" ? c : "";
  };
  const barcodes = (p: Listed) =>
    [...new Set(p.entries.map((e) => barcodeOf(e.row)).filter((b) => b !== ""))].join(", ");
  const kitchen = (p: Listed) => p.entries.some((e) => fieldsOf(e.row).kitchen === true);
</script>

<TextField label="owner.products.search" type="search" bind:value={query} />

{#each sections as section (section.id)}
  <section class="panel" aria-labelledby="pd-{section.id}">
    <h2 id="pd-{section.id}" class="type-hub-title">{t(section.title)}</h2>
    {#if !section.items.length}
      <p>{t(section.empty)}</p>
    {:else}
      <ul class="rows">
        {#each section.items as p (p.key)}
          {@const source = sourceOf(p)}
          <PricingRow
            kind={section.bundle ? "bundle" : "product"}
            name={p.name}
            edit={source
              ? {
                  label: t("hub.products.edit", { name: p.name }),
                  onclick: () => (mode = { kind: "product", item: p, bundle: section.bundle }),
                }
              : undefined}
            more={source && tracked(source.row)
              ? {
                  label: t("owner.products.threshold_edit", { name: p.name }),
                  onclick: () => (mode = { kind: "threshold", item: p }),
                }
              : undefined}
          >
            <span class="type-hub-label">{p.name}</span>
            {#if source}
              <span class="muted type-hub-micro">
                {[categoryOf(p), barcodes(p)].filter((x) => x !== "").join(" · ")}
              </span>
              {#if section.bundle && containsText(source)}
                <span class="muted type-hub-micro">{containsText(source)}</span>
              {/if}
              {#if tracked(source.row) && thresholdOf(source.row) !== undefined}
                <span class="muted type-hub-micro"
                  >{t("owner.products.threshold_value", {
                    qty: thresholdOf(source.row) ?? 0,
                  })}</span
                >
              {/if}
            {/if}
            {#if kitchen(p)}
              <span><StatusBadge tone="muted" text={t("hub.products.badge.kitchen")} /></span>
            {/if}
            <ul>
              {#each p.entries as e (e.data.branch.id)}
                <li class="muted type-hub-micro">
                  {priceLine(e)
                    ? t("owner.pricing.at", { branch: e.data.branch.name, text: priceLine(e) })
                    : e.data.branch.name}
                  {#if e.row.active === false}
                    <StatusBadge tone="muted" text={t("hub.products.badge.inactive")} />
                  {/if}
                </li>
              {/each}
            </ul>
          </PricingRow>
        {/each}
      </ul>
    {/if}
  </section>
{/each}

{#if mode?.kind === "product"}
  {#key mode.item?.key ?? ""}
    <ProductForm
      item={mode.item}
      bundle={mode.bundle}
      {data}
      {scope}
      oncancel={() => (mode = null)}
    />
  {/key}
{:else if mode?.kind === "threshold"}
  {#key mode.item.key}
    <ThresholdForm item={mode.item} {data} {scope} oncancel={() => (mode = null)} />
  {/key}
{:else}
  <div class="add">
    <Button
      label="hub.products.add"
      variant="secondary"
      onclick={() => (mode = { kind: "product", item: null, bundle: false })}
    />
    <Button
      label="owner.products.add_bundle"
      variant="secondary"
      onclick={() => (mode = { kind: "product", item: null, bundle: true })}
    />
  </div>
{/if}

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
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
