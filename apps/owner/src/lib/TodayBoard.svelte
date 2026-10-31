<script lang="ts">
  import { Button, TierLock, formatMoney, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount } from "svelte";
  import { afterNavigate } from "$app/navigation";
  import { page } from "$app/state";
  import type { OwnerBranch } from "@arena/ui/generated";
  import SetupCard from "./SetupCard.svelte";
  import TodayCard from "./TodayCard.svelte";
  import { isLive, refresh, setupBranches, shell } from "./shell.svelte";
  import {
    TICK_MS,
    attentionOf,
    busy,
    fetchDue,
    live,
    moneyLocale,
    newOpen,
    showCached,
    showable,
    type Attention,
  } from "./today.svelte";

  const PULL_PX = 80;
  /** How often a visible Today re-reads the branches while one is still in setup. */
  const BRANCH_POLL_MS = 60_000;
  const LINES: Record<string, MessageKey> = {
    shift_variance: "owner.today.attention.shift_variance",
    low_stock: "owner.today.attention.low_stock",
    credit_overdue: "owner.today.attention.credit_overdue",
    alert: "owner.today.attention.alert",
  };

  let now = $state(Date.now());
  let pullFrom: number | null = null;
  let started = false;
  const seen = new Set<string>();

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

  /** Fetches these branches now and sends their data age. */
  async function fetchNow(list: OwnerBranch[]) {
    now = Date.now();
    busy.refreshing = true;
    try {
      await fetchDue(list, true, true);
    } finally {
      busy.refreshing = false;
    }
  }

  /** The Refresh button and pull-to-refresh also re-read the branches, so a new go-live shows. */
  async function reload(force: boolean) {
    now = Date.now();
    if (!force) return fetchDue(paid, false);
    busy.refreshing = true;
    try {
      await refresh();
      await fetchDue(paid, true);
    } finally {
      busy.refreshing = false;
    }
  }

  // The first fetch of each live branch, on open and when a branch goes live while Today is open;
  // cached copies show at once. Branches in setup make no call.
  $effect(() => {
    if (!shell.ready) return;
    const fresh = paid.filter((b) => !seen.has(b.id));
    if (!fresh.length) return;
    for (const b of fresh) seen.add(b.id);
    started = true;
    void showCached(fresh).then(() => fetchNow(fresh));
  });

  // An alert's link (`/#today-<branch>`) lands on that branch's card. Cards above it grow as their
  // figures arrive, so the board keeps the card in view until every card has settled or the owner
  // scrolls.
  let target = "";
  function jump() {
    const card = target && document.getElementById(target);
    if (!card) return;
    card.scrollIntoView();
    card.focus({ preventScroll: true });
    if (paid.every((b) => live[b.id])) target = "";
  }
  $effect(() => {
    void paid.map((b) => [b.id, !!live[b.id]].join(":")).join();
    jump();
  });
  afterNavigate(() => {
    let id = "";
    try {
      id = decodeURIComponent(page.url.hash.slice(1));
    } catch {
      // A malformed escape is not a card.
    }
    target = id.startsWith("today-") ? id : "";
    jump();
  });

  onMount(() => {
    newOpen();
    const due = () => {
      now = Date.now();
      if (document.visibilityState === "visible" && started) void reload(false);
    };
    const tick = setInterval(due, TICK_MS);
    // A branch in setup goes live somewhere else (the wizard), so Today re-reads the branches.
    const poll = setInterval(() => {
      if (document.visibilityState === "visible" && setupBranches().length) void refresh();
    }, BRANCH_POLL_MS);
    document.addEventListener("visibilitychange", due);
    const stop = () => (target = "");
    for (const ev of ["wheel", "touchstart", "keydown"])
      addEventListener(ev, stop, { passive: true });
    return () => {
      for (const ev of ["wheel", "touchstart", "keydown"]) removeEventListener(ev, stop);
      clearInterval(tick);
      clearInterval(poll);
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
  {#if paid.length}
    <div class="bar">
      <Button label="owner.today.refresh" variant="secondary" onclick={() => reload(true)} />
      <span role="status" class="type-hub-label"
        >{busy.refreshing ? t("owner.today.refreshing") : ""}</span
      >
    </div>
  {/if}
  <ul class="cards">
    {#each branches as b (b.id)}
      <li>
        {#if !isLive(b)}
          <SetupCard branch={b} />
        {:else if b.tier === "free"}
          <h2 class="type-hub-label">{b.name}</h2>
          <TierLock tier="Basic" branch={b.id} />
        {:else}
          <TodayCard branch={b} {now} />
        {/if}
      </li>
    {/each}
  </ul>
  {#if paid.length}
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
  {/if}
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
