<script lang="ts">
  import { t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { page } from "$app/state";
  import { snapshotOf } from "../../../lib/operating";

  const snapshot = $derived(snapshotOf(page.data));
  const setup = $derived(snapshot?.setup);
  const groups = $derived(snapshot?.groups.length ?? 0);
  const rows: { label: MessageKey; done: boolean; href?: string }[] = $derived([
    {
      label: "hub.settings.setup.pair",
      done: !!snapshot?.devices.some((d) => d.class === "paired"),
      href: "/settings/devices",
    },
    { label: "hub.settings.setup.groups", done: groups > 0, href: "/settings/layout" },
    {
      label: "hub.settings.setup.rates",
      done: groups > 0 && setup?.groups_without_rate === 0,
      href: "/settings/layout",
    },
    {
      label: "hub.settings.setup.printer",
      done: setup?.printer === "ok" || setup?.printer === "skipped",
      href: "/settings/printer",
    },
    { label: "hub.settings.setup.test_session", done: setup?.test_session === "passed" },
  ]);
</script>

<section aria-labelledby="setup-title">
  <h2 id="setup-title" class="type-hub-title">{t("hub.settings.setup.title")}</h2>
  <p class="intro">{t("hub.settings.setup.intro")}</p>
  <ul data-testid="setup-list">
    {#each rows as r (r.label)}
      <li class="row" data-done={r.done}>
        <span class="name">{t(r.label)}</span>
        <span class="state"
          >{t(r.done ? "hub.settings.setup.done" : "hub.settings.setup.todo")}</span
        >
        {#if r.href}
          <a class="link ui-interactive" href={r.href}
            >{t("hub.settings.setup.open", { screen: t(r.label) })}</a
          >
        {/if}
      </li>
    {/each}
  </ul>
</section>

<style>
  h2,
  .intro {
    margin: 0 0 var(--space-3);
  }

  .intro {
    color: var(--text-muted);
  }

  ul {
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
    gap: var(--space-3);
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
  }

  .name {
    flex: 1;
    font-weight: var(--type-hub-title-weight);
  }

  .state {
    font-weight: var(--type-hub-title-weight);
  }

  .link {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    color: var(--text);
  }
</style>
