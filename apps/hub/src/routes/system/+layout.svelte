<script lang="ts">
  import { formatDateTime, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { Snippet } from "svelte";
  import { page } from "$app/state";
  import { onMount } from "svelte";
  import { branchLocale } from "../../lib/locale.svelte";
  import { snapshotOf } from "../../lib/operating";
  import { CYCLE_LABELS, TIER_LABELS, planOf } from "../../lib/plan";
  import { refreshAccess, service } from "../../lib/service.svelte";

  let { children }: { children: Snippet } = $props();

  /** The System sections. */
  const sections: { href: string; label: MessageKey }[] = $derived([
    { href: "/system/audit", label: "hub.system.nav.audit" },
    { href: "/system/connection", label: "hub.system.nav.connection" },
    { href: "/system/backup", label: "hub.system.nav.backup" },
    ...(service.access
      ? [{ href: "/system/service", label: "hub.system.nav.service" as const }]
      : []),
  ]);
  const section = $derived(sections.find((s) => page.url.pathname.startsWith(s.href)));
  // The plan the licence gives (Story 10.6), with its end in branch time.
  const plan = $derived(planOf(snapshotOf(page.data)));
  const planText = $derived.by(() => {
    if (!plan) return null;
    const tier = t(TIER_LABELS[plan.tier]);
    return plan.cycle
      ? t("hub.system.plan_cycle", { tier, cycle: t(CYCLE_LABELS[plan.cycle]) })
      : t("hub.system.plan", { tier });
  });
  const expiresText = $derived(
    plan?.exp
      ? t("hub.system.plan_expires", { when: formatDateTime(plan.exp, branchLocale.current?.tz) })
      : null,
  );

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
  {#if planText}
    <p class="plan type-hub-body" data-testid="system-plan">
      <span data-testid="system-plan-name">{planText}</span>
      {#if expiresText}
        <span class="muted" data-testid="system-plan-expires">{expiresText}</span>
      {/if}
    </p>
  {/if}
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

  .plan {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-4);
    margin: 0;
  }

  .plan .muted {
    color: var(--text-muted);
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
