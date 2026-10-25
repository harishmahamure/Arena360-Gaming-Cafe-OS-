<script lang="ts">
  import { t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { Snippet } from "svelte";
  import { page } from "$app/state";

  let { children }: { children: Snippet } = $props();

  /** The Settings sections, Branch first. */
  const SECTIONS: { href: string; label: MessageKey }[] = [
    { href: "/settings/branch", label: "hub.settings.nav.branch" },
    { href: "/settings/receipt", label: "hub.settings.nav.receipt" },
    { href: "/settings/screen-lock", label: "hub.settings.nav.screen_lock" },
    { href: "/settings/devices-display", label: "hub.settings.nav.devices_display" },
    { href: "/settings/layout", label: "hub.settings.nav.layout" },
    { href: "/settings/printer", label: "hub.settings.nav.printer" },
    { href: "/settings/system", label: "hub.settings.nav.system" },
  ];
  const section = $derived(SECTIONS.find((s) => page.url.pathname.startsWith(s.href)));
</script>

<svelte:head>
  <title
    >{t("hub.page_title", {
      screen: section
        ? `${t(section.label)} · ${t("hub.nav.settings")}` // i18n-ignore (a joiner)
        : t("hub.nav.settings"),
    })}</title
  >
</svelte:head>

<div class="settings">
  <h1 class="type-hub-numeric">{t("hub.nav.settings")}</h1>
  <nav class="sections" aria-label={t("hub.settings.sections")}>
    <ul>
      {#each SECTIONS as s (s.href)}
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
  .settings {
    display: grid;
    gap: var(--space-4);
    max-width: 48rem;
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
