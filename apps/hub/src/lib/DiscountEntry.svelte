<script lang="ts">
  import { Button, InlinePin, ReasonPicker, TextField, t } from "@arena/ui";
  import type { DiscountReason, IntentKind, ManualDiscount } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { dropElevation, elevate } from "./auth";
  import { parseCouponPercent } from "./coupons";
  import { DISCOUNT_REASONS, type AppliedDiscount } from "./discount";
  import { parseAmount } from "./settle";

  /**
   * The manual discount, entered in place (Story 5.22): a percentage (1–100) or an amount, then
   * a reason (with text for Other), then the manager's PIN when the signed-in member does not
   * hold `discount.manual` themselves. `onapply` gets the discount and the elevation ticket the
   * PIN earned (one approval, one settle).
   */
  let {
    kind,
    direct,
    exponent,
    resume = undefined,
    onapply,
    oncancel,
  }: {
    /** The intent the bill is settled by, which the PIN approves. */
    kind: IntentKind;
    /** The signed-in member holds `discount.manual`: no PIN is asked. */
    direct: boolean;
    exponent: number;
    /** A discount already chosen whose approval has to be asked for again: starts at the PIN. */
    resume?: ManualDiscount;
    onapply: (applied: AppliedDiscount) => void;
    oncancel: () => void;
  } = $props();

  // The dialog starts where it is told to and is not re-keyed afterwards.
  // svelte-ignore state_referenced_locally
  let step = $state((resume ? "pin" : "value") as "value" | "reason" | "pin");
  let mode = $state("percent" as "percent" | "amount");
  let text = $state("");
  let error = $state(null as MessageKey | null);
  // svelte-ignore state_referenced_locally
  let chosen = $state((resume ?? null) as ManualDiscount | null);

  function toValue() {
    const bp = mode === "percent" ? parseCouponPercent(text) : null;
    const minor = mode === "amount" ? parseAmount(text, exponent) : null;
    if (bp == null && minor == null) {
      error =
        mode === "percent"
          ? "hub.settle.discount.percent_invalid"
          : "hub.settle.discount.amount_invalid";
      return;
    }
    error = null;
    chosen = {
      percent_bp: bp ?? undefined,
      amount_minor: minor ?? undefined,
      reason: "other",
    };
    step = "reason";
  }

  function toPin(reason: string, note?: string): void {
    if (!chosen) return;
    chosen = { ...chosen, reason: reason as DiscountReason, note };
    if (direct) onapply({ discount: chosen });
    else step = "pin";
  }

  async function approve(pin: string) {
    if (!chosen) return { ok: false, message: "ui.pin.failed" } as const;
    const outcome = await elevate([{ intent_kind: kind, discount: true }], pin);
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    if (!chosen) {
      void dropElevation(outcome.ticket);
      return { ok: true } as const;
    }
    onapply({ discount: chosen, ticket: outcome.ticket });
    return { ok: true } as const;
  }
</script>

<div class="discount" data-testid="discount-entry">
  {#if step === "value"}
    <div class="modes" role="group" aria-label={t("hub.settle.discount")}>
      <Button
        label="hub.settle.discount.percent"
        variant={mode === "percent" ? "primary" : "secondary"}
        onclick={() => {
          mode = "percent";
          error = null;
        }}
      />
      <Button
        label="hub.settle.discount.amount"
        variant={mode === "amount" ? "primary" : "secondary"}
        onclick={() => {
          mode = "amount";
          error = null;
        }}
      />
    </div>
    <TextField
      label={mode === "percent"
        ? "hub.settle.discount.percent_label"
        : "hub.settle.discount.amount_label"}
      bind:value={text}
      inputmode="decimal"
      autocomplete="off"
      {error}
    />
    <div class="actions">
      <Button label="hub.settle.discount.next" onclick={toValue} />
      <Button label="ui.pin.cancel" variant="secondary" onclick={oncancel} />
    </div>
  {:else if step === "reason"}
    <ReasonPicker
      options={DISCOUNT_REASONS}
      noteFor="other"
      legend="hub.settle.discount.why"
      onconfirm={toPin}
      oncancel={() => (step = "value")}
    />
  {:else}
    <InlinePin message="hub.settle.discount.pin" onsubmit={approve} {oncancel} />
  {/if}
</div>

<style>
  .discount {
    display: grid;
    gap: var(--space-2);
  }

  .modes,
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
</style>
