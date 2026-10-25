<script lang="ts">
  import { billingEnabled } from "../billing.js";
  import { t } from "../messages/index.js";

  let {
    tier,
    billing = billingEnabled,
    href = "/account/subscription",
  }: { tier: "Basic" | "Premium"; billing?: boolean; href?: string } = $props();
</script>

<p class="lock type-hub-label" data-component="tier-lock">
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="currentColor">
    <path d="M5 7V5a3 3 0 0 1 6 0v2h1v7H4V7zm1.5 0h3V5a1.5 1.5 0 0 0-3 0z" />
  </svg>
  <span>{t("staff.tier.available_on", { tier })}</span>
  {#if billing}
    <a class="upgrade" {href}>{t("staff.tier.upgrade")}</a>
  {:else}
    <span>{t("staff.tier.soon")}</span>
  {/if}
</p>

<style>
  .lock {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin: 0;
    color: var(--tier-lock-foreground);
  }

  .upgrade {
    display: inline-flex;
    align-items: center;
    min-height: var(--space-touch-min);
    color: var(--text);
    text-decoration: underline;
  }
</style>
