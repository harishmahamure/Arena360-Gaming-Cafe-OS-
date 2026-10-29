<script lang="ts">
  import { formatMoney, t } from "@arena/ui";
  import type { SnapshotLocale } from "@arena/ui/generated";
  import { cartTotal, stockProblem, type CartLine } from "./cart";

  /** Why a line is marked: a stock shortage the Hub reported, or a bundle short a component. */
  export type LineProblem =
    | { kind: "out_of_stock" }
    | { kind: "only"; left: number }
    | { kind: "limit" }
    | { kind: "bundle_short" };

  let {
    lines,
    problems = {},
    locale = null,
    pc,
    zone = "",
    connected = false,
    busy = false,
    placed = false,
    error = null,
    onstep = () => {},
    onplace = () => {},
    onclose = () => {},
  }: {
    lines: CartLine[];
    /** Marks by product id: the reason a step or a placement was refused, shown inline. */
    problems?: Record<string, LineProblem>;
    /** The branch's money format, once the attract view has told it. */
    locale?: SnapshotLocale | null;
    /** The PC the order goes to, and its zone. */
    pc: string;
    zone?: string;
    /** The Hub link is up; with it down Place order is disabled. */
    connected?: boolean;
    /** An order is being placed. */
    busy?: boolean;
    /** The last order went through (the cart is empty and says so). */
    placed?: boolean;
    /** A whole-cart message: the counter is unreachable, or the Hub refused for another reason. */
    error?: "reconnecting" | "refused" | null;
    onstep?: (id: string, delta: 1 | -1) => void;
    onplace?: () => void;
    onclose?: () => void;
  } = $props();

  const money = (minor: number) => (locale ? formatMoney(minor, locale) : "");
  const total = $derived(cartTotal(lines));

  const words = (p: LineProblem): string => {
    switch (p.kind) {
      case "only":
        return t("player.cart.only_left", { count: p.left });
      case "limit":
        return t("player.cart.limit");
      case "bundle_short":
        return t("player.cart.bundle_short");
      default:
        return t("player.cart.out_of_stock");
    }
  };

  /** The mark on a line: the Hub's reason first, then what the live menu says about it now. */
  const problemOf = (line: CartLine): LineProblem | null =>
    problems[line.item.id] ?? stockProblem(line.item, line.qty);

  let panel: HTMLElement | undefined = $state();
  $effect(() => panel?.focus());

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      onclose();
    }
  }

  const reconnecting = $derived(!connected || error === "reconnecting");
</script>

<!-- The `cart`: a right-side panel over the Shop. It is not modal: the player can still reach the
     rest of the shell, and Esc or Close hands focus back to the Shop. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="cart"
  role="dialog"
  aria-modal="false"
  aria-labelledby="cart-title"
  tabindex="-1"
  bind:this={panel}
  onkeydown={keydown}
  data-testid="cart"
>
  <header class="head">
    <div>
      <h2 id="cart-title" class="type-kiosk-title">{t("player.cart.title")}</h2>
      <p class="to type-kiosk-body" data-testid="cart-to">
        {zone ? t("player.cart.to", { pc, zone }) : t("player.cart.to_no_zone", { pc })}
      </p>
    </div>
    <button type="button" class="close type-kiosk-label" data-testid="cart-close" onclick={onclose}>
      {t("player.cart.close")}
    </button>
  </header>

  {#if placed && lines.length === 0}
    <p class="placed type-kiosk-body" role="status" data-testid="cart-placed">
      {t("player.cart.placed")}
    </p>
  {:else if lines.length === 0}
    <p class="empty type-kiosk-body" data-testid="cart-empty">{t("player.cart.empty")}</p>
  {:else}
    <ul class="lines" aria-label={t("player.cart.lines_label")} data-testid="cart-lines">
      {#each lines as line (line.item.id)}
        {@const problem = problemOf(line)}
        <li class="line" data-testid="cart-line" data-item-id={line.item.id}>
          <div class="row">
            <span class="name type-kiosk-body" data-testid="cart-line-name">{line.item.name}</span>
            <span class="amount type-kiosk-body tabular-nums" data-testid="cart-line-amount">
              {money(line.item.price_minor * line.qty)}
            </span>
          </div>
          {#if line.item.components.length > 0}
            <ul class="parts type-kiosk-label" data-testid="cart-line-components">
              {#each line.item.components as part, index (index)}
                <li>{t("player.shop.component", { qty: part.qty * line.qty, name: part.name })}</li>
              {/each}
            </ul>
          {/if}
          <div
            class="stepper"
            role="group"
            aria-label={t("player.cart.qty", { qty: line.qty, name: line.item.name })}
          >
            <button
              type="button"
              class="step type-kiosk-title"
              aria-label={t("player.cart.fewer", { name: line.item.name })}
              data-testid="cart-less"
              onclick={() => onstep(line.item.id, -1)}
            >
              <span aria-hidden="true">−</span>
            </button>
            <span class="qty type-kiosk-title tabular-nums" data-testid="cart-qty">{line.qty}</span>
            <button
              type="button"
              class="step type-kiosk-title"
              aria-label={t("player.cart.more", { name: line.item.name })}
              data-testid="cart-more"
              onclick={() => onstep(line.item.id, 1)}
            >
              <span aria-hidden="true">+</span>
            </button>
          </div>
          {#if problem !== null}
            <p class="problem type-kiosk-label" role="alert" data-testid="cart-line-problem">
              <span aria-hidden="true">⚠</span>
              {words(problem)}
            </p>
          {/if}
        </li>
      {/each}
    </ul>

    <div class="total-row">
      <span class="type-kiosk-body">{t("player.cart.total")}</span>
      <span class="total type-hud-numeric tabular-nums" data-testid="cart-total">
        {money(total)}
      </span>
    </div>
    <p class="note type-kiosk-label" data-testid="cart-tax-note">{t("player.cart.tax_note")}</p>

    <fieldset class="pay">
      <legend class="visually-hidden">{t("player.cart.pay_label")}</legend>
      <label class="choice">
        <input type="radio" name="cart-pay" checked data-testid="cart-pay-bill" />
        <span>
          <span class="type-kiosk-body">{t("player.cart.pay_bill")}</span>
          <span class="type-kiosk-label sub" data-testid="cart-pay-note">
            {t("player.cart.pay_bill_note")}
          </span>
        </span>
      </label>
    </fieldset>

    {#if reconnecting}
      <p class="status type-kiosk-label" role="status" data-testid="cart-reconnecting">
        {t("player.cart.reconnecting")}
      </p>
    {:else if error === "refused"}
      <p class="status type-kiosk-label" role="alert" data-testid="cart-refused">
        <span aria-hidden="true">⚠</span>
        {t("player.cart.refused")}
      </p>
    {/if}

    <button
      type="button"
      class="place type-kiosk-title"
      disabled={reconnecting || busy}
      data-testid="cart-place"
      onclick={onplace}
    >
      {busy ? t("player.cart.placing") : t("player.cart.place")}
    </button>
  {/if}
</div>

<style>
  .cart {
    position: fixed;
    inset-block: var(--space-4);
    inset-inline-end: var(--space-4);
    z-index: 20;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    width: min(400px, calc(100vw - var(--space-6)));
    overflow-y: auto;
    padding: var(--space-4);
    border: var(--cart-border);
    border-radius: var(--radius-xl);
    background-color: var(--cart-background);
    color: var(--cart-foreground);
    text-align: left;
  }

  .cart:focus-visible,
  button:focus-visible,
  input:focus-visible {
    outline: 4px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-3);
  }

  h2,
  .to,
  .placed,
  .empty,
  .note,
  .status,
  .problem {
    margin: 0;
  }

  .to,
  .note,
  .sub,
  .parts {
    color: var(--text-muted);
  }

  .close,
  .step {
    min-width: var(--space-touch-min);
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: var(--text-field-border);
    border-color: var(--border-strong);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }

  .lines {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .line {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--line);
  }

  .row {
    display: flex;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .name {
    overflow-wrap: anywhere;
  }

  .parts {
    margin: 0;
    padding: 0 0 0 var(--space-5);
    list-style: none;
  }

  .stepper {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .qty {
    min-width: 2ch;
    text-align: center;
  }

  .problem {
    color: var(--text);
    font-weight: 700;
  }

  .total-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .pay {
    margin: 0;
    padding: 0;
    border: 0;
  }

  .choice {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    min-height: var(--space-touch-min);
  }

  .choice span {
    display: flex;
    flex-direction: column;
  }

  .place {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: 0;
    border-radius: var(--radius-full);
    background-color: var(--button-primary-background);
    color: var(--button-primary-foreground);
    cursor: pointer;
  }

  .place:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }
</style>
