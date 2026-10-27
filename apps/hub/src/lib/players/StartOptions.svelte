<script lang="ts">
  import { formatDateTime, formatMoney, t } from "@arena/ui";
  import type { OwnedPlanView, PlanView, SnapshotLocale } from "@arena/ui/generated";
  import { untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY, loadWallet } from "../auth";
  import { loadPlans, timeText, windowText } from "../plans";
  import type { StartChoice } from "../tile-actions.svelte";
  import { walletEvents } from "../wallet.svelte";

  let {
    playerId,
    groupId,
    locale,
    rate,
    disabled = false,
    value = $bindable({ mode: "postpaid" }),
  }: {
    playerId: string | null;
    groupId: string;
    locale: SnapshotLocale | null;
    rate: string | null;
    disabled?: boolean;
    value?: StartChoice;
  } = $props();

  let packs: OwnedPlanView[] = $state([]);
  let passes: OwnedPlanView[] = $state([]);
  let blocks: PlanView[] = $state([]);
  let balance: number | null = $state(null);
  let loaded = $state(false);
  let turn = 0;

  async function refresh() {
    const mine = ++turn;
    const [outcome, listed] = await Promise.all([
      playerId ? loadWallet(playerId) : Promise.resolve(null),
      loadPlans(),
    ]);
    if (mine !== turn) return;
    if (outcome?.kind === "signed_out" || listed.kind === "signed_out") {
      return void (await invalidate(SESSION_DEPENDENCY));
    }
    const owned = outcome?.kind === "ok" ? (outcome.value.plans ?? []) : [];
    packs = owned.filter((p) => p.plan_kind === "pack" && p.group_ids.includes(groupId));
    passes = owned.filter((p) => p.plan_kind === "pass" && p.group_ids.includes(groupId));
    blocks =
      listed.kind === "ok"
        ? listed.plans.filter(
            (p) => p.plan_kind === "block" && p.active && p.group_ids.includes(groupId),
          )
        : [];
    balance = outcome?.kind === "ok" ? outcome.value.balance_minor : null;
    const current = value;
    if (current.mode === "pack" && !packs.some((p) => p.id === current.pack_id && usable(p))) {
      value = { mode: "postpaid" };
    }
    if (current.mode === "pass" && !passes.some((p) => p.id === current.pack_id && usable(p))) {
      value = { mode: "postpaid" };
    }
    if (current.mode === "block" && !blocks.some((p) => p.id === current.plan_id)) {
      value = { mode: "postpaid" };
    }
    if (current.mode === "wallet" && (balance === null || !locale)) value = { mode: "postpaid" };
    loaded = true;
  }

  $effect(() => {
    void playerId;
    void groupId;
    void walletEvents.version;
    untrack(() => void refresh());
  });

  const date = (at: number) => (locale ? formatDateTime(at, locale.tz) : "");
  const usable = (p: OwnedPlanView) =>
    !p.expired && (p.plan_kind === "pass" || (p.minutes_left ?? 0) > 0);
  const chosen = (p: OwnedPlanView) =>
    (value.mode === "pack" || value.mode === "pass") && value.pack_id === p.id;
  const windowOf = (p: { window_start_min?: number | null; window_end_min?: number | null }) =>
    windowText(p.window_start_min ?? 0, p.window_end_min ?? 0);

  function choose(next: StartChoice) {
    if (!disabled) value = next;
  }
</script>

{#if playerId || blocks.length > 0}
  <div
    class="options"
    role="group"
    aria-labelledby="start-options-title"
    data-testid="start-options"
  >
    <p class="type-hub-label" id="start-options-title">{t("hub.session.opt.title")}</p>
    <button
      type="button"
      class="ui-interactive option type-hub-body"
      aria-pressed={value.mode === "postpaid"}
      data-testid="start-option-postpaid"
      onclick={() => choose({ mode: "postpaid" })}
    >
      {t("hub.session.opt.postpaid")}
    </button>
    {#if !loaded}
      <p class="type-hub-label" role="status">{t("hub.session.opt.loading")}</p>
    {/if}
    {#each packs as p (p.id)}
      <button
        type="button"
        class="ui-interactive option type-hub-body"
        class:expired={!usable(p)}
        aria-pressed={chosen(p)}
        aria-disabled={!usable(p)}
        data-testid="start-option-pack"
        data-expired={!usable(p)}
        onclick={() => usable(p) && choose({ mode: "pack", pack_id: p.id })}
      >
        {#if p.expired}
          <span aria-hidden="true">⚠</span>
          {t("hub.session.opt.pack_expired", { name: p.name, date: date(p.expires_at) })}
        {:else}
          {t("hub.session.opt.pack", {
            name: p.name,
            time: timeText(p.minutes_left ?? 0),
            date: date(p.expires_at),
          })}
        {/if}
      </button>
    {/each}
    {#each passes as p (p.id)}
      <button
        type="button"
        class="ui-interactive option type-hub-body"
        class:expired={!usable(p)}
        aria-pressed={chosen(p)}
        aria-disabled={!usable(p)}
        data-testid="start-option-pass"
        data-expired={!usable(p)}
        onclick={() => usable(p) && choose({ mode: "pass", pack_id: p.id })}
      >
        {#if p.expired}
          <span aria-hidden="true">⚠</span>
          {t("hub.session.opt.pack_expired", { name: p.name, date: date(p.expires_at) })}
        {:else}
          {t("hub.session.opt.pass", {
            name: p.name,
            window: windowOf(p),
            date: date(p.expires_at),
          })}
        {/if}
      </button>
    {/each}
    {#each blocks as p (p.id)}
      <button
        type="button"
        class="ui-interactive option type-hub-body tabular-nums"
        aria-pressed={value.mode === "block" && value.plan_id === p.id}
        data-testid="start-option-block"
        onclick={() => choose({ mode: "block", plan_id: p.id })}
      >
        {t("hub.session.opt.block", {
          name: p.name,
          price: locale ? formatMoney(p.price_minor, locale) : "",
          window: windowOf(p),
        })}
      </button>
    {/each}
    {#if balance !== null && locale}
      <button
        type="button"
        class="ui-interactive option type-hub-body tabular-nums"
        aria-pressed={value.mode === "wallet"}
        data-testid="start-option-wallet"
        onclick={() => choose({ mode: "wallet" })}
      >
        {rate
          ? t("hub.session.opt.wallet", { balance: formatMoney(balance, locale), rate })
          : t("hub.session.opt.wallet_no_rate", { balance: formatMoney(balance, locale) })}
      </button>
    {/if}
  </div>
{/if}

<style>
  .options {
    display: grid;
    gap: var(--space-2);
  }

  p {
    margin: 0;
  }

  .option {
    --ui-bg: var(--button-secondary-background);
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    color: var(--button-secondary-foreground);
    font: inherit;
    text-align: start;
    cursor: pointer;
  }

  .option[aria-pressed="true"] {
    border-width: 2px;
    border-color: var(--accent);
  }

  .expired {
    color: var(--text-muted);
    cursor: not-allowed;
  }
</style>
