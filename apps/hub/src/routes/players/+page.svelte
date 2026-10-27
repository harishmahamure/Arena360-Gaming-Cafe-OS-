<script lang="ts">
  import { Button, formatMoney, t } from "@arena/ui";
  import type { PlayerProfile, WalletView } from "@arena/ui/generated";
  import { onMount, tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import { SESSION_DEPENDENCY, hasPermission, loadPlayer, loadWallet } from "../../lib/auth";
  import type { MessageKey } from "@arena/ui/messages";
  import CreditBlock from "../../lib/players/CreditBlock.svelte";
  import CreateProfile from "../../lib/players/CreateProfile.svelte";
  import EditProfile from "../../lib/players/EditProfile.svelte";
  import PlayerLookup from "../../lib/players/PlayerLookup.svelte";
  import TopUp from "../../lib/players/TopUp.svelte";
  import WalletList from "../../lib/players/WalletList.svelte";
  import UndoDock from "../../lib/UndoDock.svelte";
  import { branchLocale } from "../../lib/locale.svelte";
  import { walletEvents } from "../../lib/wallet.svelte";
  import { sessionStaff } from "../../lib/operating";

  let open = $state(false);
  let prefill = $state("+91");
  let profile: PlayerProfile | null = $state(null);
  let guest = $state(false);
  let failed = $state(false);
  let lookup: PlayerLookup | undefined = $state();
  let created: string | null = $state(null);
  let editing = $state(false);
  let edited: MessageKey | null = $state(null);
  let stale = $state(false);
  let trigger: HTMLElement | undefined = $state();
  let wallet: WalletView | null = $state(null);
  let walletOpen = $state(false);
  let walletFailed = $state(false);
  let toppingUp = $state(false);

  const staff = $derived(sessionStaff(page.data));

  let turn = 0;
  let walletTurn = 0;

  async function refreshWallet(id: string) {
    const mine = ++walletTurn;
    const outcome = await loadWallet(id);
    if (mine !== walletTurn) return;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    walletFailed = outcome.kind !== "ok";
    if (outcome.kind === "ok") wallet = outcome.value;
  }

  async function reloadBalance(id: string) {
    void refreshWallet(id);
    const outcome = await loadPlayer(id);
    if (outcome.kind === "ok" && profile?.player.player_id === id) profile = outcome.value;
  }

  $effect(() => {
    const changed = walletEvents.version;
    const id = untrack(() => profile?.player.player_id);
    if (changed > 0 && id && walletEvents.playerId === id) {
      void reloadBalance(id);
    }
  });

  async function pick(id: string) {
    guest = false;
    failed = false;
    editing = false;
    edited = null;
    stale = false;
    toppingUp = false;
    walletOpen = false;
    wallet = null;
    const mine = ++turn;
    const outcome = await loadPlayer(id);
    if (mine !== turn) return;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    failed = outcome.kind !== "ok";
    profile = outcome.kind === "ok" ? outcome.value : null;
  }

  onMount(() => {
    if (page.url.searchParams.has("lookup")) lookup?.focus();
  });

  async function refresh(done: MessageKey | null) {
    const id = profile?.player.player_id;
    if (!id) return;
    const outcome = await loadPlayer(id);
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "ok") profile = outcome.value;
    failed = outcome.kind !== "ok";
    editing = done === null;
    edited = done;
    stale = done === null;
  }

  async function close(username: string | null) {
    open = false;
    created = username;
    if (username) void lookup?.search();
    await tick();
    trigger?.querySelector("button")?.focus();
  }
</script>

<UndoDock {staff} />

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.nav.players") })}</title>
</svelte:head>

<section class="players" aria-labelledby="players-title">
  <h1 class="type-hub-numeric" id="players-title">{t("hub.nav.players")}</h1>
  <p class="intro type-hub-body">{t("hub.players.intro")}</p>

  {#if hasPermission(staff, "player.view")}
    <PlayerLookup
      bind:this={lookup}
      onpick={(hit) => void pick(hit.player_id)}
      onguest={() => {
        profile = null;
        guest = true;
      }}
      oncreate={(phone) => {
        prefill = phone;
        created = null;
        open = true;
      }}
    />
  {/if}

  <div role="status" class="type-hub-label">
    {#if guest}{t("hub.players.lookup.guest_picked")}{/if}
    {#if failed}<span aria-hidden="true">⚠</span> {t("hub.players.lookup.failed")}{/if}
  </div>

  {#if profile}
    {@const p = profile.player}
    {@const c = profile.consent}
    <section class="profile" aria-labelledby="player-profile-title" data-testid="player-profile">
      <h2 class="type-hub-title" id="player-profile-title">{t("hub.players.profile.title")}</h2>
      <dl class="type-hub-body">
        <dt>{t("hub.players.name")}</dt>
        <dd>{p.name}</dd>
        <dt>{t("hub.players.username")}</dt>
        <dd>{p.username}</dd>
        <dt>{t("hub.players.phone")}</dt>
        <dd>{p.phone}</dd>
        <dt>{t("hub.wallet.balance")}</dt>
        <dd class="tabular-nums" data-testid="wallet-balance">
          {branchLocale.current ? formatMoney(profile.wallet_minor ?? 0, branchLocale.current) : ""}
        </dd>
        <dt>{t("hub.players.profile.tags")}</dt>
        <dd>{p.tags.length ? p.tags.join(", ") : t("hub.players.profile.no_tags")}</dd>
        <dt>{t("hub.players.profile.consent")}</dt>
        <dd>
          {#if c}
            {t(c.granted ? "hub.players.profile.granted" : "hub.players.profile.not_granted", {
              version: c.policy_version,
            })}
          {:else}
            {t("hub.players.profile.no_consent")}
          {/if}
        </dd>
      </dl>
      {#if !toppingUp}
        <div class="actions" data-testid="wallet-actions">
          {#if hasPermission(staff, "wallet.topup")}
            <Button label="hub.wallet.topup" onclick={() => (toppingUp = true)} />
          {/if}
          <Button
            label={walletOpen ? "hub.wallet.list.hide" : "hub.wallet.list.show"}
            variant="secondary"
            onclick={() => {
              walletOpen = !walletOpen;
              if (walletOpen) void refreshWallet(p.player_id);
            }}
          />
        </div>
      {/if}
      {#if !editing}
        <div class="actions" data-testid="player-edit">
          <Button
            label="hub.players.edit"
            onclick={() => {
              edited = null;
              stale = false;
              editing = true;
            }}
          />
        </div>
      {/if}
    </section>
    {#key p.player_id}
      <CreditBlock playerId={p.player_id} {staff} />
    {/key}
    {#if toppingUp}
      <TopUp
        playerId={p.player_id}
        ondone={() => {
          toppingUp = false;
          void reloadBalance(p.player_id);
        }}
        oncancel={() => (toppingUp = false)}
      />
    {/if}
    {#if walletOpen}
      <section aria-labelledby="wallet-title" data-testid="wallet">
        <h2 class="type-hub-title" id="wallet-title">{t("hub.wallet.title")}</h2>
        {#if walletFailed}
          <p class="type-hub-label" role="alert">
            <span aria-hidden="true">⚠</span>
            {t("hub.wallet.list.failed")}
          </p>
        {:else if wallet && branchLocale.current}
          <WalletList {wallet} locale={branchLocale.current} />
        {/if}
      </section>
    {/if}
    {#if editing}
      {#key profile.rev}
        {#if stale}<p class="type-hub-label" role="alert">
            <span aria-hidden="true">⚠</span>
            {t("hub.players.edit.stale")}
          </p>{/if}
        <EditProfile
          {staff}
          {profile}
          ondone={(message) => void refresh(message)}
          onstale={() => void refresh(null)}
          oncancel={() => {
            editing = false;
            stale = false;
          }}
        />
      {/key}
    {/if}
  {/if}

  {#if !open}
    <div class="actions" bind:this={trigger} data-testid="player-create">
      <Button
        label="hub.players.create"
        onclick={() => {
          prefill = "+91";
          created = null;
          open = true;
        }}
      />
    </div>
  {/if}

  <p class="status type-hub-label" role="status" data-testid="player-status">
    {#if created}{t("hub.players.created", { username: created })}{/if}
    {#if edited}{t(edited)}{/if}
  </p>

  {#if open}
    {#key prefill}
      <CreateProfile
        {staff}
        phone={prefill}
        oncreated={(username) => void close(username)}
        oncancel={() => void close(null)}
      />
    {/key}
  {/if}
</section>

<style>
  .players {
    display: grid;
    gap: var(--space-3);
    max-width: 48rem;
  }

  h1,
  p {
    margin: 0;
  }

  .intro {
    color: var(--text-muted);
  }

  .profile {
    display: grid;
    gap: var(--space-2);
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-2) var(--space-4);
    margin: 0;
  }

  dt {
    color: var(--text-muted);
  }

  dd {
    margin: 0;
  }

  .status {
    min-height: 1lh;
  }
</style>
