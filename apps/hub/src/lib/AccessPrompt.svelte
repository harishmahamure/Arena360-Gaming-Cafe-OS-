<script lang="ts">
  import { Button, t } from "@arena/ui";
  import { bridge, refreshAccess, service } from "./service.svelte";

  const show = $derived(service.access !== null && !service.access.asked);

  function allow() {
    bridge()?.requestAccess();
    refreshAccess();
  }

  function skip() {
    bridge()?.skipAccess();
    refreshAccess();
  }
</script>

{#if show}
  <section class="access" aria-labelledby="access-title" data-testid="hub-access">
    <h2 class="type-hub-title" id="access-title">{t("hub.access.title")}</h2>
    <p class="type-hub-body">{t("hub.access.body")}</p>
    <div class="actions">
      <Button label="hub.access.allow" onclick={allow} />
      <Button label="hub.access.skip" variant="secondary" onclick={skip} />
    </div>
  </section>
{/if}

<style>
  .access {
    display: grid;
    gap: var(--space-3);
    max-width: 60ch;
  }

  h2,
  p {
    margin: 0;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
