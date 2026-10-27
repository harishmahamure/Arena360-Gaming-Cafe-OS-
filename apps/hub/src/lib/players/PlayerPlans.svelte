<script lang="ts">
  import { formatDateTime, t } from "@arena/ui";
  import type { OwnedPlanView } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import { SESSION_DEPENDENCY, loadWallet } from "../auth";
  import { branchLocale } from "../locale.svelte";
  import { snapshotOf } from "../operating";
  import { timeText, windowText } from "../plans";
  import { walletEvents } from "../wallet.svelte";

  let { playerId, version = 0 }: { playerId: string; version?: number } = $props();

  const KIND: Record<string, MessageKey> = {
    pack: "hub.plans.kind.pack",
    pass: "hub.plans.kind.pass",
  };

  const locale = $derived(branchLocale.current);
  const groups = $derived(snapshotOf(page.data)?.groups ?? []);
  const groupName = (id: string) => groups.find((g) => g.id === id)?.name ?? id;

  let owned: OwnedPlanView[] = $state([]);
  let loaded = $state(false);
  let failed = $state(false);
  let turn = 0;

  async function refresh() {
    const mine = ++turn;
    const outcome = await loadWallet(playerId);
    if (mine !== turn) return;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    failed = outcome.kind !== "ok";
    if (outcome.kind === "ok") owned = outcome.value.plans ?? [];
    loaded = true;
  }

  $effect(() => {
    void playerId;
    void version;
    void walletEvents.version;
    untrack(() => void refresh());
  });

  const left = (p: OwnedPlanView) => (p.plan_kind === "pack" ? timeText(p.minutes_left ?? 0) : "");
</script>

<section class="owned" aria-labelledby="owned-title" data-testid="owned-plans">
  <h2 class="type-hub-title" id="owned-title">{t("hub.plans.owned.title")}</h2>
  {#if failed}
    <p class="type-hub-label" role="alert">
      <span aria-hidden="true">⚠</span>
      {t("hub.plans.owned.failed")}
    </p>
  {:else if loaded && owned.length === 0}
    <p class="type-hub-body" data-testid="owned-empty">{t("hub.plans.owned.empty")}</p>
  {:else}
    <ul class="rows">
      {#each owned as p (p.id)}
        <li class="row" class:expired={p.expired} data-testid="owned-row" data-expired={p.expired}>
          <span class="type-hub-title">{p.name}</span>
          <span class="badge type-hub-label">{t(KIND[p.plan_kind] ?? "hub.plans.kind.pack")}</span>
          {#if p.expired}
            <span class="badge type-hub-label" data-testid="owned-expired"
              >{t("hub.plans.owned.expired")}</span
            >
          {/if}
          {#if p.plan_kind === "pack"}
            <span class="type-hub-label" data-testid="owned-left">
              {t(p.expired ? "hub.plans.owned.unused" : "hub.plans.owned.left", {
                time: left(p),
              })}
            </span>
          {:else}
            <span class="type-hub-label" data-testid="owned-window">
              {t("hub.plans.owned.plays", {
                window: windowText(p.window_start_min ?? 0, p.window_end_min ?? 0),
              })}
            </span>
          {/if}
          <span class="type-hub-label" data-testid="owned-expiry">
            {t("hub.plans.owned.expires", {
              date: locale ? formatDateTime(p.expires_at, locale.tz) : "",
            })}
          </span>
          <span class="type-hub-label">
            {t("hub.plans.for_groups", { names: p.group_ids.map(groupName).join(", ") })}
          </span>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .owned {
    display: grid;
    gap: var(--space-2);
    max-width: 48rem;
  }

  h2,
  p {
    margin: 0;
  }

  .rows {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background-color: var(--surface);
  }

  .expired {
    color: var(--text-muted);
  }

  .badge {
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
  }
</style>
