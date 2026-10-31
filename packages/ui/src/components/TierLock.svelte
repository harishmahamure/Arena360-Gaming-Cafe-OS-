<script lang="ts">
  import { t } from "../messages/index.js";
  import { upgradeContext, upgradeUrl } from "../upgrade.js";

  // `branch` is the cloud id of the branch the lock is about; `href` overrides the picker link.
  let {
    tier,
    branch = undefined,
    href = undefined,
  }: { tier: "Basic" | "Premium"; branch?: string; href?: string } = $props();

  const ctx = upgradeContext();
  const link = $derived(
    href ??
      upgradeUrl(
        ctx.base,
        tier,
        branch,
        ctx.returnHere && typeof location !== "undefined" ? location.href : undefined,
      ),
  );
</script>

<p class="lock type-hub-label" data-component="tier-lock">
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="currentColor">
    <path d="M5 7V5a3 3 0 0 1 6 0v2h1v7H4V7zm1.5 0h3V5a1.5 1.5 0 0 0-3 0z" />
  </svg>
  <span>{t("staff.tier.available_on", { tier })}</span>
  <a class="upgrade" href={link}>{t("staff.tier.upgrade")}</a>
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
