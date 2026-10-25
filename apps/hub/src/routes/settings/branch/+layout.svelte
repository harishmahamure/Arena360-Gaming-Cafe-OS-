<script lang="ts">
  import { t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { Snippet } from "svelte";
  import { page } from "$app/state";
  import { adsNeeded, snapshotOf } from "../../../lib/operating";

  let { children }: { children: Snippet } = $props();

  const showAds = $derived(adsNeeded(snapshotOf(page.data)));
  const links: { href: string; label: MessageKey }[] = $derived([
    { href: "/settings/branch", label: "hub.settings.branch.nav.locale" },
    { href: "/settings/branch/hours", label: "hub.settings.branch.nav.hours" },
    { href: "/settings/branch/service", label: "hub.settings.branch.nav.service" },
    { href: "/settings/branch/tax", label: "hub.settings.branch.nav.tax" },
    ...(showAds
      ? [{ href: "/settings/branch/ads", label: "hub.settings.branch.nav.ads" as MessageKey }]
      : []),
  ]);
</script>

<div class="branch-settings">
  <nav class="subsections" aria-label={t("hub.settings.branch.sections")}>
    <ul>
      {#each links as l (l.href)}
        <li>
          <a
            class="subsection ui-interactive type-hub-label"
            href={l.href}
            aria-current={page.url.pathname === l.href ? "page" : undefined}>{t(l.label)}</a
          >
        </li>
      {/each}
    </ul>
  </nav>
  {@render children()}
</div>

<style>
  .branch-settings {
    display: grid;
    gap: var(--space-4);
  }

  .subsections ul {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .subsection {
    --ui-bg: transparent;
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border-bottom: 2px solid transparent;
    color: var(--text-muted);
    text-decoration: none;
  }

  .subsection[aria-current="page"] {
    border-bottom-color: var(--accent);
    color: var(--text);
    font-weight: var(--type-hub-numeric-weight);
  }
</style>
