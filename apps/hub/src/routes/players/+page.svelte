<script lang="ts">
  import { Button, t } from "@arena/ui";
  import { tick } from "svelte";
  import { page } from "$app/state";
  import CreateProfile from "../../lib/players/CreateProfile.svelte";
  import { sessionStaff } from "../../lib/operating";

  let open = $state(false);
  let created: string | null = $state(null);
  let trigger: HTMLElement | undefined = $state();

  const staff = $derived(sessionStaff(page.data));

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

  {#if !open}
    <div class="actions" bind:this={trigger} data-testid="player-create">
      <Button
        label="hub.players.create"
        onclick={() => {
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

  .status {
    min-height: 1lh;
  }
</style>
