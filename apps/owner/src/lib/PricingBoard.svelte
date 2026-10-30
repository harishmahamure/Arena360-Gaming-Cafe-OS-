<script lang="ts">
  import { Button, TabControl, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, untrack } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import CouponsPanel from "./CouponsPanel.svelte";
  import { ALL, dataOf } from "./pricing";
  import RatesPlansPanel from "./RatesPlansPanel.svelte";
  import RulesPanel from "./RulesPanel.svelte";
  import { age, shell } from "./shell.svelte";
  import { TICK_MS, fetchDue, live, showCached, showable } from "./today.svelte";

  const TABS = [
    { id: "plans", label: "owner.pricing.tab.plans" },
    { id: "rules", label: "owner.pricing.tab.rules" },
    { id: "coupons", label: "owner.pricing.tab.coupons" },
  ] as const satisfies readonly { id: string; label: MessageKey }[];
  type TabId = (typeof TABS)[number]["id"];

  // The branches whose prices Manage can change: live and on a paid plan.
  const paid = $derived(showable(shell.branches));
  let branchId = $state(ALL);
  const picked = $derived(
    branchId === ALL || paid.length < 2 ? paid : paid.filter((b) => b.id === branchId),
  );
  // One branch in scope is that branch; several are "all branches".
  const scope = $derived(picked.length === 1 ? picked[0].id : ALL);
  const data = $derived(dataOf(picked));
  const waiting = $derived(picked.some((b) => !live[b.id]));
  // Nothing could be read, and nothing is cached.
  const failed = $derived(picked.length > 0 && picked.every((b) => live[b.id]?.failed));

  const fromUrl = (): TabId => {
    const wanted = page.url.searchParams.get("tab");
    return TABS.find((x) => x.id === wanted)?.id ?? "plans";
  };
  let tab: string = $state(fromUrl());
  $effect(() => {
    tab = fromUrl();
  });

  // The selected tab is in the URL (`?tab=`), without adding history entries.
  function select(id: string) {
    const url = new URL(page.url.href);
    url.searchParams.set("tab", id);
    void goto(url, { replace: true, reset: false });
  }

  let now = $state(Date.now());
  const ago = (ms: number) => {
    const [key, n] = age(new Date(ms).toISOString(), now);
    return t(key, { n });
  };

  async function read(force: boolean) {
    await showCached(paid);
    await fetchDue(paid, force);
  }

  onMount(() => {
    // Opened without a (valid) `?tab=`: say which tab it is.
    if (page.url.searchParams.get("tab") !== tab) select(tab);
    const id = setInterval(() => {
      now = Date.now();
      if (document.visibilityState === "visible") void read(false);
    }, TICK_MS);
    return () => clearInterval(id);
  });

  // The first read is forced; a branch that turns live or paid while the board is open is read at once.
  let first = true;
  $effect(() => {
    void paid.map((b) => b.id).join();
    const force = first;
    first = false;
    void untrack(() => read(force));
  });
</script>

{#if paid.length}
  <section class="pricing" aria-label={t("owner.pricing.tabs")} data-component="pricing-board">
    {#if paid.length > 1}
      <div class="field">
        <label class="label" for="pricing-branch">{t("owner.pricing.branch")}</label>
        <select id="pricing-branch" class="ui-interactive select" bind:value={branchId}>
          <option value={ALL}>{t("owner.pricing.branch.all")}</option>
          {#each paid as b (b.id)}<option value={b.id}>{b.name}</option>{/each}
        </select>
      </div>
    {/if}
    <ul class="ages">
      {#each data as d (d.branch.id)}
        <li class="type-hub-label" data-branch={d.branch.id}>
          {d.asOf === null
            ? t("owner.pricing.age_none", { branch: d.branch.name })
            : t("owner.pricing.age", { branch: d.branch.name, age: ago(d.asOf) })}
        </li>
      {/each}
    </ul>
    <TabControl
      items={[...TABS]}
      label="owner.pricing.tabs"
      bind:selected={tab}
      onchange={select}
    />
    <div
      role="tabpanel"
      aria-label={t(TABS.find((x) => x.id === tab)?.label ?? "owner.pricing.tab.plans")}
      class="panels"
    >
      {#if failed}
        <p>{t("owner.unavailable")}</p>
        <Button label="common.retry" variant="secondary" onclick={() => void read(true)} />
      {:else if waiting && !data.some((d) => d.cat)}
        <p>{t("owner.pricing.loading")}</p>
      {:else if tab === "rules"}
        <RulesPanel {data} {scope} {now} />
      {:else if tab === "coupons"}
        <CouponsPanel {data} {scope} />
      {:else}
        <RatesPlansPanel {data} {scope} />
      {/if}
    </div>
  </section>
{/if}

<style>
  .pricing {
    --status-badge-background: var(--surface);
    display: grid;
    gap: var(--space-4);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .label {
    font-weight: var(--type-hub-title-weight);
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
    max-width: 100%;
  }
  .ages {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-1);
    color: var(--text-muted);
  }
  .panels {
    display: grid;
    gap: var(--space-5);
    min-width: 0;
  }
  p {
    margin: 0;
  }
</style>
