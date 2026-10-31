<script lang="ts">
  import { t } from "@arena/ui";
  import type { AddOns, Band, Tier } from "@arena/ui/generated";
  import { money, snapshotPrice, type Cycle } from "./tierPrice";

  const MAX_PACKS = 10;

  let {
    tier,
    band,
    cycle,
    addOns = $bindable(),
  }: { tier: Tier; band: Band; cycle: Cycle; addOns: AddOns } = $props();

  const id = $props.id();

  /** "₹50 / month" for an add-on at the selected cycle, or an empty string without a price. */
  function each(item: "addon_devices_20" | "addon_tunnel"): string {
    const p = snapshotPrice(band, item, cycle);
    if (!p) return "";
    return t(cycle === "yearly" ? "pricing.price.year" : "pricing.price.month", {
      price: money(p.minor, p.currency),
    });
  }

  function setPacks(raw: string) {
    const n = Math.trunc(Number(raw));
    addOns.devices_20 = Number.isFinite(n) ? Math.min(MAX_PACKS, Math.max(0, n)) : 0;
  }
</script>

<fieldset class="addons" data-testid="addons">
  <legend>{t("onboarding.addons.title")}</legend>

  <div class="row">
    <label for="{id}-devices" class="name">{t("onboarding.addons.devices")}</label>
    <input
      id="{id}-devices"
      class="qty tabular-nums"
      type="number"
      min="0"
      max={MAX_PACKS}
      step="1"
      inputmode="numeric"
      aria-describedby="{id}-devices-help"
      value={addOns.devices_20}
      onchange={(e) => {
        setPacks(e.currentTarget.value);
        e.currentTarget.value = String(addOns.devices_20);
      }}
    />
    <p class="help" id="{id}-devices-help">
      {t("onboarding.addons.devices_help", { price: each("addon_devices_20") })}
    </p>
  </div>

  <div class="row">
    {#if tier === "premium"}
      <span class="name">{t("onboarding.addons.tunnel")}</span>
      <p class="help" data-testid="tunnel-included">{t("onboarding.addons.tunnel_included")}</p>
    {:else}
      <label class="check name">
        <input
          type="checkbox"
          checked={addOns.tunnel === 1}
          aria-describedby="{id}-tunnel-help"
          onchange={(e) => (addOns.tunnel = e.currentTarget.checked ? 1 : 0)}
        />
        <span>{t("onboarding.addons.tunnel")}</span>
      </label>
      <p class="help" id="{id}-tunnel-help">
        {t("onboarding.addons.tunnel_help", { price: each("addon_tunnel") })}
      </p>
    {/if}
  </div>

  <div class="row">
    <label class="check name">
      <input type="checkbox" disabled checked={false} aria-describedby="{id}-login-help" />
      <span>{t("onboarding.addons.owner_login")}</span>
    </label>
    <p class="help" id="{id}-login-help" data-testid="owner-login-soon">
      {t("onboarding.addons.owner_login_soon")}
    </p>
  </div>
</fieldset>

<style>
  .addons {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
    margin: 0;
    padding: var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    background-color: var(--card-background);
  }

  legend {
    padding: 0 var(--space-1);
    font-size: var(--type-hub-title-size);
    font-weight: var(--type-hub-title-weight);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
  }

  .name {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
    font-weight: var(--type-hub-title-weight);
  }

  .check input {
    width: 1.5rem;
    height: 1.5rem;
    margin: 0;
  }

  .qty {
    box-sizing: border-box;
    width: 5rem;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--text-field-background);
    color: inherit;
    font: inherit;
  }

  .help {
    flex-basis: 100%;
    margin: 0;
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
    line-height: var(--type-hub-label-line-height);
  }
</style>
