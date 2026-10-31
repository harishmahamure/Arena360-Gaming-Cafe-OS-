<script lang="ts">
  import { onMount, tick } from "svelte";
  import { t, type MessageKey } from "../messages/index.js";
  import BrandMark from "./BrandMark.svelte";
  import ContrastSwitch from "./ContrastSwitch.svelte";
  import LinkButton from "./LinkButton.svelte";

  /**
   * The marketing header: wordmark, Product · Pricing · Downloads · Help · FAQ and the one
   * "Get started". Below 768 px the links move into a full-screen menu behind a hamburger; the menu
   * traps focus, closes on Esc or a link, and hands focus back to the hamburger.
   */
  let { startHref, current = "" }: { startHref: string; current?: string } = $props();

  const links: { href: string; label: MessageKey }[] = [
    { href: "/product", label: "site.nav.product" },
    { href: "/pricing", label: "site.nav.pricing" },
    { href: "/downloads", label: "site.nav.downloads" },
    { href: "/help", label: "site.nav.help" },
    { href: "/faq", label: "site.nav.faq" },
  ];

  let open = $state(false);
  let hamburger: HTMLButtonElement | undefined = $state();
  let menu: HTMLDivElement | undefined = $state();

  const here = (href: string) => current === href || current.startsWith(href + "/");

  const FOCUSABLE = 'a[href], button:not([disabled]), [tabindex]:not([tabindex="-1"])'; // i18n-ignore

  async function show() {
    open = true;
    await tick();
    menu?.querySelector<HTMLElement>(FOCUSABLE)?.focus();
  }

  function hide(refocus = true) {
    open = false;
    if (refocus) hamburger?.focus();
  }

  function onkeydown(event: KeyboardEvent) {
    if (!open) return;
    if (event.key === "Escape") {
      event.preventDefault();
      hide();
      return;
    }
    if (event.key !== "Tab" || !menu) return;
    const items = [...menu.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const first = items[0];
    const last = items[items.length - 1];
    if (!first || !last) return;
    if (!menu.contains(document.activeElement)) {
      event.preventDefault();
      first.focus();
    } else if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  // The menu is a phone-width control: widening the window past it closes it.
  onMount(() => {
    const wide = window.matchMedia?.("(min-width: 768px)");
    const close = (event: MediaQueryListEvent) => {
      if (event.matches) hide(false);
    };
    wide?.addEventListener("change", close);
    return () => wide?.removeEventListener("change", close);
  });

  $effect(() => {
    if (!open) return;
    const before = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      document.body.style.overflow = before;
    };
  });
</script>

<svelte:window {onkeydown} />

<div class="bar" data-component="site-header">
  <a class="brand" href="/" aria-label={t("site.header.home")}>
    <BrandMark size={32} role="presentation" aria-hidden="true" aria-label={undefined} />
    <span class="wordmark" aria-hidden="true">{t("app.name")}</span>
  </a>
  <nav class="links" aria-label={t("site.nav.main")}>
    {#each links as link (link.href)}
      <a href={link.href} aria-current={here(link.href) ? "page" : undefined}>{t(link.label)}</a>
    {/each}
  </nav>
  <div class="actions">
    <LinkButton href={startHref} label="site.cta.get_started" />
    <button
      bind:this={hamburger}
      type="button"
      class="ui-interactive hamburger"
      aria-label={t("site.nav.menu_open")}
      aria-haspopup="dialog"
      aria-expanded={open}
      onclick={show}
    >
      <svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false">
        <path d="M3 6h18M3 12h18M3 18h18" stroke="currentColor" stroke-width="2" fill="none" />
      </svg>
    </button>
  </div>
</div>

{#if open}
  <div
    bind:this={menu}
    class="menu"
    role="dialog"
    aria-modal="true"
    aria-label={t("site.nav.menu_title")}
  >
    <div class="menu-bar">
      <a class="brand" href="/" onclick={() => hide(false)} aria-label={t("site.header.home")}>
        <BrandMark size={32} role="presentation" aria-hidden="true" aria-label={undefined} />
        <span class="wordmark" aria-hidden="true">{t("app.name")}</span>
      </a>
      <div class="actions">
        <LinkButton href={startHref} label="site.cta.get_started" />
        <button
          type="button"
          class="ui-interactive hamburger"
          aria-label={t("site.nav.menu_close")}
          onclick={() => hide()}
        >
          <svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false">
            <path d="M5 5l14 14M19 5L5 19" stroke="currentColor" stroke-width="2" fill="none" />
          </svg>
        </button>
      </div>
    </div>
    <nav class="menu-links" aria-label={t("site.nav.main")}>
      {#each links as link (link.href)}
        <a
          href={link.href}
          aria-current={here(link.href) ? "page" : undefined}
          onclick={() => hide(false)}>{t(link.label)}</a
        >
      {/each}
    </nav>
    <div class="menu-foot">
      <ContrastSwitch />
    </div>
  </div>
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-5);
    box-sizing: border-box;
    height: var(--space-web-header-height-phone);
    padding: 0 var(--space-web-gutter-phone);
  }

  .brand {
    display: inline-flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    text-decoration-line: none;
  }

  .wordmark {
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  .links {
    display: none;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-inline-start: auto;
  }

  .hamburger {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: var(--space-touch-min);
    min-width: var(--space-touch-min);
    padding: 0;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    color: var(--text);
    cursor: pointer;
  }

  .menu {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    background-color: var(--bg);
    color: var(--text);
  }

  .menu-bar {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    box-sizing: border-box;
    height: var(--space-web-header-height-phone);
    padding: 0 var(--space-web-gutter-phone);
  }

  .menu-links {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-5) var(--space-web-gutter-phone);
  }

  .menu-links a {
    display: flex;
    align-items: center;
    min-height: var(--space-touch-min);
    padding: var(--space-2) 0;
    font-size: var(--type-web-display-size);
    font-weight: var(--type-web-display-weight);
    line-height: var(--type-web-display-line-height);
    overflow-wrap: anywhere;
    text-decoration-line: none;
  }

  .menu-links a[aria-current="page"] {
    text-decoration-line: underline;
  }

  .menu-foot {
    padding: var(--space-5) var(--space-web-gutter-phone);
  }

  @media (min-width: 768px) {
    .bar {
      gap: var(--space-6);
      height: var(--space-web-header-height);
      padding: 0 var(--space-web-gutter);
    }

    .links {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-1) var(--space-5);
      margin-inline-start: auto;
    }

    .links a {
      display: inline-flex;
      align-items: center;
      min-height: var(--space-touch-min);
      min-width: var(--space-touch-min);
    }

    .links a[aria-current="page"] {
      font-weight: var(--type-hub-title-weight);
    }

    .actions {
      margin-inline-start: 0;
    }

    .hamburger {
      display: none;
    }
  }
</style>
