<script lang="ts">
  import { t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { Snippet } from "svelte";
  import { page } from "$app/state";
  import { onMount } from "svelte";
  import { refreshAccess, service } from "../../lib/service.svelte";

  let { children }: { children: Snippet } = $props();

  /** The System sections. */
  const sections: { href: string; label: MessageKey }[] = $derived([
    { href: "/system/audit", label: "hub.system.nav.audit" },
    ...(service.access
      ? [{ href: "/system/service", label: "hub.system.nav.service" as const }]
      : []),
  ]);
  const section = $derived(sections.find((s) => page.url.pathname.startsWith(s.href)));

  onMount(refreshAccess);
</script>

<svelte:head>
  <title
    >{t("hub.page_title", {
      screen: section
        ? `${t(section.label)} · ${t("hub.nav.system")}` // i18n-ignore (a joiner)
        : t("hub.nav.system"),
    })}</title
  >
</svelte:head>

<div class="system">
  <h1 class="type-hub-numeric">{t("hub.nav.system")}</h1>
  <nav class="sections" aria-label={t("hub.system.sections")}>
    <ul>
      {#each sections as s (s.href)}
        <li>
          <a
            class="section ui-interactive type-hub-label"
            href={s.href}
            aria-current={section?.href === s.href ? "page" : undefined}>{t(s.label)}</a
          >
        </li>
      {/each}
    </ul>
  </nav>
  <div class="body">
    {@render children()}
  </div>
</div>

<style>
  .system {
    display: grid;
    gap: var(--space-4);
    max-width: 80rem;
  }

  h1 {
    margin: 0;
  }

  .sections ul {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .section {
    --ui-bg: transparent;
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    color: var(--text-muted);
    text-decoration: none;
  }

  /* The current section: filled, full ink and heavier (never colour alone). */
  .section[aria-current="page"] {
    --ui-bg: var(--hub-rail-item-active);
    color: var(--text);
    font-weight: var(--type-hub-numeric-weight);
  }
</style>
