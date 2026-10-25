<script lang="ts">
  import { t } from "../messages/index.js";

  let { tier }: { tier: "Basic" | "Premium" } = $props();
</script>

<p class="lock type-hub-label" data-component="tier-lock">
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="currentColor">
    <path d="M5 7V5a3 3 0 0 1 6 0v2h1v7H4V7zm1.5 0h3V5a1.5 1.5 0 0 0-3 0z" />
  </svg>
  {t("staff.tier.available_on", { tier })}
</p>

<style>
  .lock {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    color: var(--tier-lock-foreground);
  }
</style>
