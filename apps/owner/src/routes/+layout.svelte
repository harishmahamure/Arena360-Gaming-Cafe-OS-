<script lang="ts">
  import "@arena/ui/base.css";
  import { StatusBadge, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { page } from "$app/state";
  import { onMount, type Snippet } from "svelte";
  import { age, refresh, shell } from "../lib/shell.svelte";

  let { children }: { children: Snippet } = $props();

  const NAV = [
    {
      href: "/",
      label: "owner.nav.today",
      d: "M3,11l9-8,9,8v9a1,1,0,0,1-1,1h-5v-6H9v6H4a1,1,0,0,1-1-1z",
    },
    { href: "/reports", label: "owner.nav.reports", d: "M4,20V10h4v10zm6,0V4h4v16zm6,0v-7h4v7z" },
    {
      href: "/alerts",
      label: "owner.nav.alerts",
      d: "M12,22a2,2,0,0,0,2-2h-4a2,2,0,0,0,2,2zm6-6V11a6,6,0,0,0-5-5.9V4a1,1,0,0,0-2,0v1.1A6,6,0,0,0,6,11v5l-2,2v1h16v-1z",
    },
    { href: "/manage", label: "owner.nav.manage", d: "M4,6h10v2H4zm0,5h16v2H4zm0,5h8v2H4z" },
    {
      href: "/account",
      label: "owner.nav.account",
      d: "M12,12a4,4,0,1,0,0-8,4,4,0,0,0,0,8zm-8,9a8,8,0,0,1,16,0z",
    },
  ] as const satisfies readonly { href: string; label: MessageKey; d: string }[];

  const current = (href: string) =>
    href === "/"
      ? page.url.pathname === "/"
      : page.url.pathname === href || page.url.pathname.startsWith(href + "/");

  onMount(() => {
    refresh();
    const down = () => (shell.offline = true);
    addEventListener("online", refresh);
    addEventListener("offline", down);
    return () => {
      removeEventListener("online", refresh);
      removeEventListener("offline", down);
    };
  });
</script>

<div class="shell">
  <header class="header">
    <a class="brand" href="/">{t("app.name")}</a>
    {#if shell.cafe}<span class="cafe">{shell.cafe}</span>{/if}
    <div class="status" role="status">
      {#if shell.offline}
        <StatusBadge state="offline" label="owner.offline" />
        {#if shell.asOf}
          {@const [key, n] = age(shell.asOf)}
          <span class="type-hub-label">{t("owner.updated", { age: t(key, { n }) })}</span>
        {/if}
      {/if}
    </div>
  </header>
  <nav class="nav" aria-label={t("owner.nav.label")}>
    {#each NAV as item (item.href)}
      <a
        class="item ui-interactive"
        class:active={current(item.href)}
        href={item.href}
        aria-current={current(item.href) ? "page" : undefined}
      >
        <svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" fill="currentColor"
          ><path d={item.d} /></svg
        >
        <span class="label">{t(item.label)}</span>
      </a>
    {/each}
  </nav>
  <main class="main">{@render children()}</main>
</div>

<style>
  :global(body) {
    margin: 0;
  }

  .shell {
    display: grid;
    min-height: 100dvh;
    grid-template-rows: auto 1fr;
    grid-template-columns: minmax(0, 1fr);
    grid-template-areas: "header" "main";
    padding-bottom: var(--space-owner-header-height);
  }

  .header {
    grid-area: header;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--space-owner-header-height);
    padding: 0 var(--space-owner-gutter-phone);
    box-sizing: border-box;
  }

  .brand {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  .cafe {
    color: var(--text-muted);
  }

  .status {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-left: auto;
  }

  .main {
    grid-area: main;
    box-sizing: border-box;
    width: 100%;
    max-width: 1200px;
    padding: var(--space-4) var(--space-owner-gutter-phone) var(--space-owner-section-gap);
  }

  .main :global(a) {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
  }

  :global([data-theme="light"]) .nav {
    --owner-nav-background: var(--surface);
    --owner-nav-item-active: var(--bg);
  }

  .nav {
    box-sizing: border-box;
    position: fixed;
    inset: auto 0 0 0;
    z-index: 1;
    display: flex;
    height: var(--space-owner-header-height);
    background-color: var(--owner-nav-background);
    border-top: var(--owner-nav-tab-bar-border);
  }

  .item {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    min-width: var(--space-touch-min);
    text-decoration: none;
    font-size: var(--type-hub-micro-size);
    font-weight: var(--type-hub-micro-weight);
    letter-spacing: var(--type-hub-micro-letter-spacing);
    color: var(--text-muted);
  }

  .item.active {
    background-color: var(--owner-nav-item-active);
    color: var(--text);
  }

  @media (min-width: 768px) {
    .shell {
      padding-bottom: 0;
      grid-template-columns: var(--space-owner-sidebar-width) minmax(0, 1fr);
      grid-template-areas: "nav header" "nav main";
    }

    .header {
      padding: 0 var(--space-owner-gutter);
    }

    .main {
      padding: var(--space-4) var(--space-owner-gutter) var(--space-owner-section-gap);
    }

    .nav {
      position: sticky;
      top: 0;
      inset: auto;
      grid-area: nav;
      flex-direction: column;
      height: 100dvh;
      padding: var(--space-4) 0;
      border-top: 0;
      border-right: var(--owner-nav-sidebar-border);
    }

    .item {
      flex: none;
      flex-direction: row;
      justify-content: flex-start;
      gap: var(--space-3);
      min-height: var(--space-touch-min);
      padding: 0 var(--space-5);
      font-size: var(--type-hub-label-size);
      font-weight: var(--type-hub-label-weight);
      letter-spacing: normal;
    }
  }
</style>
