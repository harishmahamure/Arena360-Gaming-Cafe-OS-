<script lang="ts">
  import { Button, t } from "@arena/ui";
  import { onMount } from "svelte";
  import CommandRow from "./CommandRow.svelte";
  import { POLL_MS, cmd, groupOf, refreshCommands } from "./commands.svelte";

  let now = $state(Date.now());

  const groups = $derived(
    (["today", "earlier"] as const)
      .map((g) => [g, cmd.rows.filter((r) => groupOf(r, now) === g)] as const)
      .filter(([, rows]) => rows.length),
  );

  async function tick() {
    now = Date.now();
    await refreshCommands();
  }

  // Polls while the page is visible, and reads at once when it becomes visible again.
  onMount(() => {
    void tick();
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") void tick();
    }, POLL_MS);
    const seen = () => {
      if (document.visibilityState === "visible") void tick();
    };
    document.addEventListener("visibilitychange", seen);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", seen);
    };
  });
</script>

<section class="commands" aria-labelledby="cmd-list-title">
  <h2 id="cmd-list-title" class="type-hub-title">{t("owner.cmd.list.title")}</h2>
  <div class="visually-hidden" role="status" aria-live="polite">{cmd.announcement}</div>
  {#if !cmd.loaded && cmd.failed}
    <p>{t("owner.cmd.list.failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={tick} />
  {:else if !cmd.loaded}
    <p>{t("owner.loading")}</p>
  {:else if !groups.length}
    <p>{t("owner.cmd.list.empty")}</p>
  {:else}
    {#each groups as [g, rows] (g)}
      <h3 class="type-hub-label">
        {t(g === "today" ? "owner.cmd.group.today" : "owner.cmd.group.earlier")}
      </h3>
      <ul class="rows">
        {#each rows as row (row.id)}
          <CommandRow {row} {now} />
        {/each}
      </ul>
    {/each}
  {/if}
</section>

<style>
  .commands {
    display: grid;
    gap: var(--space-3);
  }
  h2,
  h3,
  p {
    margin: 0;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--line);
  }
</style>
