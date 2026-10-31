<script lang="ts">
  import { Button, TabControl, formatDateTime, t } from "@arena/ui";
  import type { AccountSubscription } from "@arena/ui/generated";
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { signInUrl } from "../../../lib/api";
  import InvoicesPanel from "../../../lib/InvoicesPanel.svelte";
  import { refresh } from "../../../lib/shell.svelte";
  import SubscriptionCard from "../../../lib/SubscriptionCard.svelte";
  import {
    CYCLES,
    cycleOf,
    loadPlans,
    ordered,
    stateText,
    type Cycle,
  } from "../../../lib/subscription";

  // An Upgrade link for a branch that already has a plan lands here on that branch (Story 10.5).
  const wanted = $derived(page.url.searchParams.get("branch"));
  let cycle: Cycle = $state(cycleOf(page.url.searchParams.get("cycle")));
  $effect(() => {
    cycle = cycleOf(page.url.searchParams.get("cycle"));
  });

  let plans = $state(null as AccountSubscription | null);
  let loaded = $state(false);
  let announcement = $state("");
  const branches = $derived(ordered(plans?.branches ?? [], wanted));

  // The Monthly | Yearly choice is in the URL, without adding history entries.
  function pick(id: string) {
    const url = new URL(page.url.href);
    url.searchParams.set("cycle", id);
    void goto(url, { replace: true, reset: false });
  }

  async function fetchPlans() {
    const res = await loadPlans();
    if (res === "signed-out") return location.assign(signInUrl());
    if (res) {
      // A state that changed since the last look is said once.
      const before = new Map((plans?.branches ?? []).map((b) => [b.branch_id, b.state]));
      for (const b of res.data.branches) {
        if (plans && before.get(b.branch_id) !== b.state)
          announcement = t("owner.subscription.announce", {
            branch: b.branch_name,
            state: stateText(
              b.state,
              b.current_period_end
                ? formatDateTime(b.current_period_end, undefined, { dateStyle: "medium" })
                : "",
            ),
          });
      }
      plans = res.data;
    }
    loaded = true;
  }

  // A change was stored, withdrawn or found stale: the cards are read again and the result said once.
  async function changed(message: string) {
    await fetchPlans();
    if (message) announcement = message;
  }

  async function retry() {
    await refresh();
    await fetchPlans();
  }

  onMount(() => void fetchPlans());
</script>

<svelte:head
  ><title>{t("app.title", { surface: t("owner.account.subscription") })}</title></svelte:head
>

<h1 class="type-web-display">{t("owner.account.subscription")}</h1>
<div class="visually-hidden" role="status" aria-live="polite">{announcement}</div>
{#if plans}
  <TabControl
    items={[...CYCLES]}
    label="owner.subscription.cycle.label"
    bind:selected={cycle}
    onchange={pick}
  />
  <ul class="list">
    {#each branches as plan (plan.branch_id)}
      <SubscriptionCard {plan} {cycle} current={plan.branch_id === wanted} onchanged={changed} />
    {/each}
  </ul>
  <InvoicesPanel announce={(text) => (announcement = text)} />
{:else if loaded}
  <p>{t("owner.unavailable")}</p>
  <Button label="common.retry" variant="secondary" onclick={retry} />
{/if}
<svelte:window onfocus={fetchPlans} />
<p><a href="/account">{t("owner.account.back")}</a></p>

<style>
  .list {
    display: grid;
    grid-template-columns: 1fr;
    gap: var(--space-4);
    margin: var(--space-4) 0 0;
    padding: 0;
    list-style: none;
  }

  @media (min-width: 768px) {
    .list {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
</style>
