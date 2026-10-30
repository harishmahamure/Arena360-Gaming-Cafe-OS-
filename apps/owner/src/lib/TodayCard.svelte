<script lang="ts">
  import { Button, Card, StatusBadge, formatMoney, t } from "@arena/ui";
  import type { OwnerBranch } from "@arena/ui/generated";
  import { fetchWaiting, queuedText, waiting, whatOf } from "./commands.svelte";
  import { entryOf } from "./live";
  import MetricFigure from "./MetricFigure.svelte";
  import { age } from "./shell.svelte";
  import { LATE_MS, fetchLive, live, moneyLocale } from "./today.svelte";

  let { branch, now }: { branch: OwnerBranch; now: number } = $props();

  const state = $derived(live[branch.id]);
  const data = $derived(state?.data ?? null);
  const summary = $derived(data?.summary ?? null);
  const offline = $derived(!!data && !data.branch_online);
  const queued = $derived(waiting[branch.id] ?? []);
  const entry = $derived(entryOf(branch));

  // An offline branch lists what it has yet to apply; re-read whenever its figures are.
  $effect(() => {
    void state;
    if (offline) void fetchWaiting(branch.id);
  });
  const money = $derived(moneyLocale(summary?.currency ?? data?.currency ?? "INR"));

  const ago = (ms: number) => {
    const [key, n] = age(new Date(ms).toISOString(), now);
    return t(key, { n });
  };
  const asOf = $derived(summary ? t("owner.today.as_of", { age: ago(summary.as_of) }) : "");
  const late = $derived(!!summary && summary.open && now - summary.as_of > LATE_MS);

  // The latest counted shift (newest closing first), and whether the Hub flagged it.
  const counted = $derived(
    (summary?.shifts ?? [])
      .filter((s) => s.variance_minor !== undefined && s.closed_at !== undefined)
      .sort((a, b) => (b.closed_at ?? 0) - (a.closed_at ?? 0))[0],
  );
  const over = $derived(
    !!counted &&
      !!summary?.attention.some((a) => a.kind === "shift_variance" && a.ref === counted.shift_id),
  );
  const variance = $derived.by(() => {
    const v = counted?.variance_minor;
    if (v === undefined) return t("owner.today.variance.none");
    if (v === 0) return t("owner.today.variance.exact");
    const amount = (v > 0 ? "+" : "") + formatMoney(v, money);
    return t(v > 0 ? "owner.today.variance.over" : "owner.today.variance.short", { amount });
  });
</script>

<Card>
  <section class="branch" aria-labelledby="today-{branch.id}" class:offline>
    <div class="head">
      <h2 id="today-{branch.id}" class="type-hub-title" tabindex="-1">{branch.name}</h2>
      {#if offline}<StatusBadge state="offline" label="owner.today.offline" />{/if}
    </div>
    {#if !state}
      <p>{t("owner.loading")}</p>
    {:else if state.failed}
      <p>{t("owner.unavailable")}</p>
      <Button label="common.retry" variant="secondary" onclick={() => fetchLive(branch)} />
    {:else if !summary || !data}
      <p>{t("owner.today.waiting")}</p>
    {:else}
      <dl class="figures">
        <MetricFigure
          label="owner.today.status"
          value={t(summary.open ? "owner.today.open" : "owner.today.closed")}
          age={asOf}
          {late}
          muted={offline}
        />
        <MetricFigure
          label="owner.today.sessions"
          value={String(summary.sessions_live)}
          age={asOf}
          {late}
          muted={offline}
        />
        <MetricFigure
          label="owner.today.revenue"
          value={formatMoney(summary.revenue_so_far_minor, money)}
          age={asOf}
          {late}
          muted={offline}
        />
        <MetricFigure
          label="owner.today.cash"
          value={formatMoney(summary.cash_in_drawer_minor, money)}
          age={asOf}
          {late}
          muted={offline}
        />
        <MetricFigure label="owner.today.variance" age={asOf} {late} muted={offline}>
          <span class="variance" class:over>
            <span class="value tabular-nums">{variance}</span>
            {#if over}<StatusBadge state="warning" label="owner.today.variance.threshold" />{/if}
          </span>
        </MetricFigure>
        <MetricFigure
          label="owner.today.staff"
          value={summary.staff_on_shift.join(", ") || t("owner.today.staff.none")}
          age={asOf}
          {late}
          muted={offline}
        />
        <MetricFigure
          label="owner.today.sync"
          value={data.last_heartbeat_at === null
            ? t("owner.today.sync.never")
            : ago(data.last_heartbeat_at)}
          muted={offline}
        />
      </dl>
    {/if}
    {#if entry === "platform"}
      <p class="type-hub-label live-note">{t("owner.live.needs_windows")}</p>
    {:else}
      <!-- On a phone the sidebar entry is not there: the card carries the way in. -->
      <div class="live-phone">
        <a href="/live?branch={branch.id}">{t("owner.live.open")}</a>
      </div>
    {/if}
    {#if offline && queued.length}
      <div class="waiting">
        <h3 class="type-hub-label">{t("owner.cmd.today.waiting")}</h3>
        <ul>
          {#each queued as c (c.id)}
            <li class="type-hub-micro">{whatOf(c.kind, c.payload)} · {queuedText(c, now)}</li>
          {/each}
        </ul>
      </div>
    {/if}
  </section>
</Card>

<style>
  .branch {
    display: grid;
    gap: var(--space-4);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
  h2 {
    margin: 0;
  }
  .figures {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: var(--space-4);
    margin: 0;
  }
  .waiting h3 {
    margin: 0 0 var(--space-2);
  }
  .waiting ul {
    margin: 0;
    padding-left: var(--space-5);
    display: grid;
    gap: var(--space-1);
  }
  .live-note {
    margin: 0;
    color: var(--text-muted);
  }
  .live-phone {
    justify-self: start;
  }
  .offline h2 {
    color: var(--text-muted);
  }
  .variance {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    padding-left: var(--space-2);
    border-left: 4px solid transparent;
  }
  .variance.over {
    border-left-color: var(--warning-text, var(--warning));
  }
  .value {
    font-family: var(--type-hub-numeric-family);
    font-size: var(--type-hub-numeric-size);
    font-weight: var(--type-hub-numeric-weight);
    line-height: var(--type-hub-numeric-line-height);
  }
  .offline .value {
    color: var(--text-muted);
  }
  @media (min-width: 768px) {
    .live-phone {
      display: none;
    }
    .figures {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }
</style>
