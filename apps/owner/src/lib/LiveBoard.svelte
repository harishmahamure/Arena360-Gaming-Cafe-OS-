<script lang="ts">
  import { Button, Card, FloorTile, StatusBadge, TierLock, t } from "@arena/ui";
  import type { LiveFloor, OwnerBranch } from "@arena/ui/generated";
  import { onDestroy, onMount, untrack } from "svelte";
  import { page } from "$app/state";
  import {
    LIVE_AGE_MS,
    byZone,
    connectLive,
    entryOf,
    fetchGrant,
    hubNow,
    tileView,
    type LiveConnection,
  } from "./live";
  import { isLive, refresh, shell } from "./shell.svelte";

  type View = "connecting" | "live" | "offline";

  const branches = $derived((shell.branches ?? []).filter(isLive));
  let branchId = $state("");
  const picked = $derived(branches.find((b) => b.id === branchId) ?? branches[0]);
  const entry = $derived(picked ? entryOf(picked) : null);

  let view: View = $state("connecting");
  let tier = $state(false);
  let platform = $state(false);
  let floor: LiveFloor | null = $state(null);
  let receivedAt = $state(0);
  let now = $state(Date.now());

  /** The tile's second line: who is playing, then "Remote" for a device outside the café network. */
  const remoteNote = (who: string | undefined, remote: boolean | undefined) =>
    [who, remote ? t("hub.floor.remote") : undefined].filter(Boolean).join(" · ") || undefined;
  let connection: LiveConnection | null = null;
  // Each start gets a number; an answer for an older start is dropped.
  let ticket = 0;

  const stop = () => {
    ticket++;
    connection?.close();
    connection = null;
  };

  async function start(b: OwnerBranch) {
    stop();
    floor = null;
    tier = false;
    platform = false;
    if (entryOf(b) !== "ready") return;
    const mine = ticket;
    view = "connecting";
    const answer = await fetchGrant(b.id);
    if (mine !== ticket) return;
    if (answer.state === "signed-out") return void refresh();
    if (answer.state === "tier") {
      tier = true;
      return;
    }
    if (answer.state === "platform") {
      platform = true;
      return;
    }
    if (answer.state !== "ok") {
      view = "offline";
      return;
    }
    connection = connectLive(answer.grant, () => fetchGrant(b.id), {
      open: () => mine === ticket && (view = "live"),
      floor: (f) => {
        if (mine !== ticket) return;
        floor = f;
        receivedAt = Date.now();
        view = "live";
      },
      closed: () => mine === ticket && (view = "offline"),
    });
  }

  // The first branch, or the one the Today card asked for, once the branches are known.
  $effect(() => {
    if (branchId || !branches.length) return;
    const wanted = page.url.searchParams.get("branch");
    branchId = branches.some((b) => b.id === wanted) ? (wanted as string) : branches[0].id;
  });
  // Opens (or closes) the view whenever the picked branch changes.
  // Keyed on the id: a branch refresh that replaces the objects must not reconnect.
  const pickedId = $derived(picked?.id);
  $effect(() => {
    const id = pickedId;
    const b = untrack(() => picked);
    if (id && b) void start(b);
    else stop();
  });

  onMount(() => {
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  onDestroy(stop);

  const groups = $derived(floor ? byZone(floor) : []);
  const at = $derived(floor ? hubNow(floor, receivedAt, now) : 0);
  const age = $derived(Math.max(0, now - receivedAt));
  const fresh = $derived(age <= LIVE_AGE_MS);
</script>

<svelte:head><title>{t("app.title", { surface: t("owner.nav.live") })}</title></svelte:head>

<h1 class="type-web-display">{t("owner.nav.live")}</h1>
<div class="board">
  {#if !shell.ready}
    <p>{t("owner.loading")}</p>
  {:else if !shell.branches}
    <p>{t("owner.unavailable")}</p>
    <Button label="common.retry" variant="secondary" onclick={refresh} />
  {:else if !picked}
    <TierLock tier="Premium" />
  {:else}
    {#if branches.length > 1}
      <label class="pick">
        <span class="type-hub-label">{t("owner.live.branch")}</span>
        <select bind:value={branchId}>
          {#each branches as b (b.id)}<option value={b.id}>{b.name}</option>{/each}
        </select>
      </label>
    {/if}
    {#if entry === "platform" || platform}
      <p>{t("owner.live.needs_windows")}</p>
    {:else if entry === "tier" || tier}
      <TierLock tier="Premium" />
    {:else if view === "offline"}
      <Card>
        <div class="offline" role="status">
          <StatusBadge state="offline" label="owner.today.offline" />
          <a href="/#today-{picked.id}">{t("owner.live.back")}</a>
        </div>
      </Card>
    {:else if !floor}
      <p role="status">{t("owner.live.connecting")}</p>
    {:else}
      <p class="age" role="status" data-testid="live-age">
        {#if fresh}
          <StatusBadge state="free" label="owner.live.live" />
        {:else}
          <StatusBadge state="warning" label="owner.live.live" />
          <span class="type-hub-micro"
            >{t("owner.live.updated", { n: Math.floor(age / 1000) })}</span
          >
        {/if}
      </p>
      {#each groups as g (g.id)}
        <section aria-labelledby="zone-{g.id || 'none'}">
          <h2 id="zone-{g.id || 'none'}" class="type-hub-label">
            {g.name ?? t("hub.floor.unassigned")}
          </h2>
          <ul class="tiles">
            {#each g.tiles as tile (tile.device_id)}
              {@const v = tileView(tile, at)}
              <li data-device={tile.device_id}>
                <FloorTile
                  name={tile.label}
                  state={v.state}
                  label={v.label}
                  time={v.time}
                  warning={v.warning}
                  secondary={remoteNote(v.who, tile.remote)}
                  tabindex={-1}
                />
              </li>
            {/each}
          </ul>
        </section>
      {:else}
        <p>{t("owner.live.empty")}</p>
      {/each}
    {/if}
  {/if}
</div>

<style>
  /* The tile and badge colours are drawn for the Hub's dark floor; here they sit on the owner's own surface. */
  .board {
    --floor-tile-background: var(--surface);
    --status-badge-background: var(--surface);
  }
  .pick {
    display: grid;
    gap: var(--space-1);
    max-width: 24rem;
    margin-bottom: var(--space-4);
  }
  .offline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }
  .age {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  section {
    margin-bottom: var(--space-5);
  }
  h2 {
    margin: 0 0 var(--space-2);
  }
  .tiles {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(10rem, 1fr));
    gap: var(--space-3);
  }
</style>
