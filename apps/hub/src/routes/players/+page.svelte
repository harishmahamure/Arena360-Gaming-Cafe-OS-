<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { PlayerProfile } from "@arena/ui/generated";
  import { onMount, tick } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import { SESSION_DEPENDENCY, loadPlayer } from "../../lib/auth";
  import CreateProfile from "../../lib/players/CreateProfile.svelte";
  import PlayerLookup from "../../lib/players/PlayerLookup.svelte";
  import { sessionStaff } from "../../lib/operating";

  let open = $state(false);
  let prefill = $state("+91");
  let profile: PlayerProfile | null = $state(null);
  let guest = $state(false);
  let failed = $state(false);
  let lookup: PlayerLookup | undefined = $state();
  let created: string | null = $state(null);
  let trigger: HTMLElement | undefined = $state();

  const staff = $derived(sessionStaff(page.data));

  async function pick(id: string) {
    guest = false;
    failed = false;
    const outcome = await loadPlayer(id);
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    failed = outcome.kind !== "ok";
    profile = outcome.kind === "ok" ? outcome.value : null;
  }

  onMount(() => {
    if (page.url.searchParams.has("lookup")) lookup?.focus();
  });

  async function close(username: string | null) {
    open = false;
    created = username;
    await tick();
    trigger?.querySelector("button")?.focus();
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.nav.players") })}</title>
</svelte:head>

<section class="players" aria-labelledby="players-title">
  <h1 class="type-hub-numeric" id="players-title">{t("hub.nav.players")}</h1>
  <p class="intro type-hub-body">{t("hub.players.intro")}</p>

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
    </section>
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
  </p>

  {#if open}
    <CreateProfile
      {staff}
      phone={prefill}
      oncreated={(username) => void close(username)}
      oncancel={() => void close(null)}
    />
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
