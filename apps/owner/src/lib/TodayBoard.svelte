<script lang="ts">
  import { Button, TierLock, formatMoney, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import TodayCard from "./TodayCard.svelte";
  import { shell } from "./shell.svelte";
  import {
    TICK_MS,
    attentionOf,
    busy,
    fetchDue,
    moneyLocale,
    newOpen,
    showCached,
    showable,
    type Attention,
  } from "./today.svelte";

  const PULL_PX = 80;
  const LINES: Record<string, MessageKey> = {
    shift_variance: "owner.today.attention.shift_variance",
    low_stock: "owner.today.attention.low_stock",
    credit_overdue: "owner.today.attention.credit_overdue",
    alert: "owner.today.attention.alert",
  };

  let now = $state(Date.now());
  let pullFrom: number | null = null;
  let started = false;

  const branches = $derived(shell.branches ?? []);
  const paid = $derived(showable(shell.branches));
  const attention = $derived(attentionOf(paid));

  const line = (a: Attention) => {
    const p = a.params;
    const key = LINES[a.kind] ?? "owner.today.attention.other";
    const money = moneyLocale(a.currency);
    const v = Number(p.variance_minor);
    return t(key, {
      branch: a.branch,
      staff: p.staff ?? "",
      variance: p.variance_minor
        ? t(v > 0 ? "owner.today.variance.over" : "owner.today.variance.short", {
            amount: (v > 0 ? "+" : "") + formatMoney(v, money),
          })
        : "",
      product: p.product ?? "",
      qty: p.qty ?? "",
      amount: p.outstanding_minor ? formatMoney(Number(p.outstanding_minor), money) : "",
      days: p.oldest_age_days ?? "",
      kind: p.alert_kind ?? a.kind,
    });
  };

  async function reload(force: boolean, report = false) {
    now = Date.now();
    if (force) busy.refreshing = true;
    try {
      await fetchDue(paid, force, report);
    } finally {
      busy.refreshing = false;
    }
  }

  // The first fetch once the branches are known; cached copies show at once.
  $effect(() => {
    if (!shell.ready || started || !paid.length) return;
    started = true;
    void showCached(paid).then(() => reload(true, true));
  });

  onMount(() => {
    newOpen();
    const due = () => {
      now = Date.now();
      if (document.visibilityState === "visible" && started) void reload(false);
    };
    const tick = setInterval(due, TICK_MS);
    document.addEventListener("visibilitychange", due);
    return () => {
      clearInterval(tick);
      document.removeEventListener("visibilitychange", due);
    };
  });

  const down = (e: TouchEvent) => {
    pullFrom = scrollY <= 0 ? e.touches[0].clientY : null;
  };
  const up = (e: TouchEvent) => {
    if (pullFrom !== null && e.changedTouches[0].clientY - pullFrom > PULL_PX) void reload(true);
    pullFrom = null;
  };
</script>

<div class="board" role="presentation" ontouchstart={down} ontouchend={up}>
  <div class="bar">
    <Button label="owner.today.refresh" variant="secondary" onclick={() => reload(true)} />
    <span role="status" class="type-hub-label"
      >{busy.refreshing ? t("owner.today.refreshing") : ""}</span
    >
  </div>
  <ul class="cards">
    {#each branches as b (b.id)}
      <li>
        {#if b.tier === "free"}
          <h2 class="type-hub-label">{b.name}</h2>
          <TierLock tier="Basic" />
        {:else}
          <TodayCard branch={b} {now} />
        {/if}
      </li>
    {/each}
  </ul>
  <section aria-labelledby="attention-h">
    <h2 id="attention-h" class="type-hub-title">{t("owner.today.attention")}</h2>
    {#if attention.length}
      <ul class="attention">
        {#each attention as a (a.branch + a.kind + a.ref)}
          <li>{line(a)}</li>
        {/each}
      </ul>
    {:else}
      <p>{t("owner.today.attention.none")}</p>
    {/if}
  </section>
</div>

<style>
  .board {
    display: grid;
    gap: var(--space-owner-section-gap);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .cards {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: var(--space-owner-section-gap);
  }
  .attention {
    margin: 0;
    padding-left: var(--space-5);
    display: grid;
    gap: var(--space-2);
  }
  h2 {
    margin: 0 0 var(--space-2);
  }
</style>
