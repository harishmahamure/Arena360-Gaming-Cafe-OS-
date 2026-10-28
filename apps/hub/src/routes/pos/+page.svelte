<script lang="ts">
  import { Button, formatMoney, t } from "@arena/ui";
  import type { PlayerHit, Product } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { SESSION_DEPENDENCY } from "../../lib/auth";
  import { pushBack } from "../../lib/back";
  import { catalog } from "../../lib/catalog.svelte";
  import { branchLocale } from "../../lib/locale.svelte";
  import { isTextEntry } from "../../lib/nav";
  import PlayerLookup from "../../lib/players/PlayerLookup.svelte";
  import {
    ALL,
    FAVOURITES,
    ScanBurst,
    cartCount,
    cartTotal,
    categoriesOf,
    findByBarcode,
    lineShortage,
    step,
    unavailableTag,
    visibleProducts,
    type CartLine,
    type Refusal,
  } from "../../lib/pos";
  import { loadProducts, priceText } from "../../lib/products";
  import { viewport } from "../../lib/viewport.svelte";

  let products: Product[] = $state([]);
  let loading = $state(true);
  let loadFailed = $state(false);
  let loads = 0;

  let category: string | null = $state(null);
  let query = $state("");
  let notice: { text: string; tone: "ok" | "refusal" } | null = $state(null);

  let cart: CartLine[] = $state([]);
  let refusals: Record<string, string> = $state({});
  let attached = $state<PlayerHit | null>(null);
  let choosing = $state(false);
  let cartOpen = $state(false);

  let sheetHeading: HTMLElement | undefined = $state();
  let barButton: HTMLButtonElement | undefined = $state();
  let changeButton: HTMLButtonElement | undefined = $state();
  const burst = new ScanBurst();

  const byId = $derived(new Map(products.map((p) => [p.id, p])));
  const categories = $derived(categoriesOf(products));
  const strip = $derived([FAVOURITES, ALL, ...categories]);
  const active = $derived(
    category !== null && strip.includes(category)
      ? category
      : products.some((p) => p.favourite)
        ? FAVOURITES
        : ALL,
  );
  const shown = $derived(visibleProducts(products, active, query));
  const lines = $derived(
    cart.flatMap((line) => {
      const product = byId.get(line.productId);
      return product ? [{ line, product, short: lineShortage(cart, product, byId) }] : [];
    }),
  );
  const count = $derived(cartCount(cart));
  const total = $derived(cartTotal(cart, byId));
  const locale = $derived(branchLocale.current);
  const money = (minor: number) => (locale ? formatMoney(minor, locale) : priceText(minor, 2));
  const sheet = $derived(viewport.phone && cartOpen);
  const who = $derived(attached ? attached.name || attached.username || attached.phone : null);

  const categoryLabel = (entry: string): string =>
    entry === FAVOURITES ? t("hub.pos.favourites") : entry === ALL ? t("hub.pos.all") : entry;

  async function load() {
    const mine = ++loads;
    const outcome = await loadProducts();
    if (mine !== loads) return;
    loading = false;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    loadFailed = outcome.kind !== "ok";
    if (outcome.kind !== "ok") return;
    products = outcome.products;
    const known = new Set(outcome.products.map((p) => p.id));
    cart = cart.filter((l) => known.has(l.productId));
  }

  onMount(() => void load());
  $effect(() => {
    if (catalog.version) untrack(() => void load());
  });

  function refusalText({ name, left, cap }: Refusal): string {
    return cap ? t("hub.pos.cart.cap", { name, left }) : t("hub.pos.cart.limit", { name, left });
  }

  /** Puts one of `product` in the cart (a tile, a scan); a refusal goes in the notice. */
  function add(product: Product) {
    const tag = unavailableTag(product);
    if (tag) {
      notice = {
        tone: "refusal",
        text: t("hub.pos.cannot_add", { name: product.name, reason: t(tag).toLowerCase() }),
      };
      return;
    }
    const moved = step(cart, product, 1, byId);
    if (moved.refusal) {
      notice = { tone: "refusal", text: refusalText(moved.refusal) };
      return;
    }
    cart = moved.cart;
    refusals = {};
    notice = { tone: "ok", text: t("hub.pos.added", { name: product.name }) };
  }

  /** The ± steppers: a step up past what is on hand is refused under that line. */
  function nudge(product: Product, delta: number) {
    const moved = step(cart, product, delta, byId);
    if (moved.refusal) {
      refusals = { [product.id]: refusalText(moved.refusal) };
      return;
    }
    cart = moved.cart;
    refusals = {};
    notice = null;
    if (cart.length === 0) cartOpen = false;
  }

  function scan(code: string) {
    const wanted = code.trim();
    if (!wanted) return;
    const product = findByBarcode(products, wanted);
    if (!product) {
      notice = { tone: "refusal", text: t("hub.pos.barcode_unknown", { code: wanted }) };
      return;
    }
    add(product);
  }

  function onSearchKey(event: KeyboardEvent) {
    if (event.key !== "Enter") return;
    event.preventDefault();
    if (!query.trim()) return;
    scan(query);
    query = "";
  }

  /** A keyboard-wedge scan while focus is on the POS outside a text field. */
  function onWindowKey(event: KeyboardEvent) {
    if (event.ctrlKey || event.altKey || event.metaKey || event.defaultPrevented) return;
    if (isTextEntry(document.activeElement)) return burst.reset();
    const code = burst.key(event.key, event.timeStamp);
    if (code === null) return;
    event.preventDefault();
    scan(code);
  }

  async function pick(hit: PlayerHit | null) {
    attached = hit;
    choosing = false;
    await tick();
    changeButton?.focus();
  }

  async function openCart() {
    cartOpen = true;
    await tick();
    sheetHeading?.focus();
  }

  async function closeCart() {
    cartOpen = false;
    await tick();
    barButton?.focus();
  }

  $effect(() => {
    if (!sheet) return;
    viewport.sheetFull = true;
    const off = pushBack(() => void closeCart());
    return () => {
      viewport.sheetFull = false;
      off();
    };
  });

  function onSheetKey(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    if (choosing) choosing = false;
    else void closeCart();
  }
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.nav.pos") })}</title>
</svelte:head>

<svelte:window onkeydown={onWindowKey} />

{#snippet cartPanel()}
  <section class="cart" data-component="cart" data-testid="cart" aria-labelledby="cart-title">
    <div class="cart-head">
      <h2 class="type-hub-title" id="cart-title">{t("hub.pos.cart.title")}</h2>
      <p class="who type-hub-label">
        <span data-testid="cart-for">{who ?? t("hub.pos.cart.walkup")}</span>
        <button
          type="button"
          class="ui-interactive link type-hub-label"
          data-testid="cart-change"
          bind:this={changeButton}
          onclick={() => (choosing = true)}>{t("hub.pos.cart.change")}</button
        >
      </p>
    </div>

    {#if choosing}
      <div class="chooser" data-testid="cart-chooser">
        <h3 class="type-hub-label">{t("hub.pos.player.title")}</h3>
        <PlayerLookup onpick={(hit) => void pick(hit)} onguest={() => void pick(null)} />
        <Button
          label="hub.pos.player.cancel"
          variant="secondary"
          onclick={() => (choosing = false)}
        />
      </div>
    {/if}

    {#if lines.length === 0}
      <p class="empty type-hub-body" data-testid="cart-empty">{t("hub.pos.cart.empty")}</p>
    {:else}
      <ul class="lines" aria-label={t("hub.pos.cart.lines")} data-testid="cart-lines">
        {#each lines as { line, product, short } (product.id)}
          <li class="line" data-testid="cart-line" data-product-id={product.id}>
            <div class="line-main">
              <span class="line-name type-hub-body">{product.name}</span>
              <span class="line-price type-hub-numeric tabular-nums"
                >{money(product.price_minor * line.qty)}</span
              >
            </div>
            <div class="stepper">
              <button
                type="button"
                class="ui-interactive step"
                aria-label={t("hub.pos.cart.fewer", { name: product.name })}
                data-testid="line-dec"
                onclick={() => nudge(product, -1)}>−</button
              >
              <span class="qty type-hub-numeric tabular-nums" data-testid="line-qty"
                >{line.qty}</span
              >
              <button
                type="button"
                class="ui-interactive step"
                aria-label={t("hub.pos.cart.more", { name: product.name })}
                data-testid="line-inc"
                onclick={() => nudge(product, 1)}>+</button
              >
            </div>
            {#if product.bundle && product.components.length}
              <ul class="parts type-hub-label" data-testid="line-parts">
                {#each product.components as part (part.product_id)}
                  <li>{t("hub.pos.cart.part", { qty: part.qty * line.qty, name: part.name })}</li>
                {/each}
              </ul>
            {/if}
            {#if refusals[product.id]}
              <p class="refusal type-hub-label" data-testid="line-refusal">
                <span aria-hidden="true">⚠</span>
                {refusals[product.id]}
              </p>
            {:else if short}
              <p class="refusal type-hub-label" data-testid="line-short">
                <span aria-hidden="true">⚠</span>
                {t("hub.pos.cart.short", { name: short.name, left: short.left })}
              </p>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section
    class="settle"
    data-component="settle-bar"
    data-testid="settle-bar"
    aria-labelledby="pos-settle-title"
  >
    <h3 class="type-hub-title" id="pos-settle-title">{t("hub.pos.settle.title")}</h3>
    {#if lines.length > 0}
      <p class="total">
        <span class="type-hub-label">{t("hub.settle.total")}</span>
        <span class="type-hub-numeric tabular-nums" data-testid="settle-total">{money(total)}</span>
      </p>
    {/if}
    <Button
      label="hub.pos.settle.submit"
      disabled={true}
      disabledReason={lines.length === 0 ? "hub.pos.settle.empty" : "hub.pos.settle.soon"}
    />
  </section>
{/snippet}

<section class="pos" class:phone={viewport.phone} data-testid="pos">
  {#if !sheet}
    <div class="catalogue" data-component="pos-product-grid" data-testid="pos-product-grid">
      <h1 class="type-hub-numeric">{t("hub.nav.pos")}</h1>

      <div
        class="strip"
        role="group"
        aria-label={t("hub.pos.categories")}
        data-testid="pos-categories"
      >
        {#each strip as entry (entry)}
          <button
            type="button"
            class="ui-interactive chip type-hub-label"
            aria-pressed={active === entry && !query.trim()}
            data-category={entry === FAVOURITES ? "favourites" : entry === ALL ? "all" : entry}
            onclick={() => {
              category = entry;
              query = "";
            }}>{categoryLabel(entry)}</button
          >
        {/each}
      </div>

      <div class="search">
        <label class="type-hub-label" for="pos-search">{t("hub.pos.search.label")}</label>
        <input
          id="pos-search"
          class="ui-interactive control"
          type="text"
          autocomplete="off"
          data-pos-search
          bind:value={query}
          oninput={() => (notice = null)}
          onkeydown={onSearchKey}
        />
      </div>

      <p
        class="notice type-hub-label"
        class:refusal={notice?.tone === "refusal"}
        role="status"
        data-testid="pos-notice"
      >
        {#if notice}
          {#if notice.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
          {notice.text}
        {/if}
      </p>

      <div class="results">
        {#if loading}
          <p class="type-hub-body" role="status">{t("hub.pos.loading")}</p>
        {:else if loadFailed && products.length === 0}
          <div class="failed" role="alert">
            <p class="type-hub-body">{t("hub.pos.load_failed")}</p>
            <Button label="hub.pos.retry" variant="secondary" onclick={() => void load()} />
          </div>
        {:else if shown.length === 0}
          <p class="type-hub-body" data-testid="pos-none">
            {query.trim() ? t("hub.pos.no_match", { query: query.trim() }) : t("hub.pos.empty")}
          </p>
        {:else}
          <ul class="tiles" aria-label={t("hub.pos.grid")} data-testid="pos-tiles">
            {#each shown as product (product.id)}
              {@const tag = unavailableTag(product)}
              <li>
                <button
                  type="button"
                  class="ui-interactive tile"
                  data-component="hub-tile"
                  data-testid="pos-tile"
                  data-product-id={product.id}
                  disabled={tag !== null}
                  onclick={() => add(product)}
                >
                  <span class="photo" class:dim={tag !== null} aria-hidden="true">
                    {#if product.image}
                      <img src={product.image} alt="" />
                    {:else}
                      <span class="initial type-hub-numeric">{product.name.slice(0, 1)}</span>
                    {/if}
                  </span>
                  <span class="tile-name type-hub-body">
                    {product.name}
                    {#if product.favourite}
                      <span role="img" aria-label={t("hub.pos.favourite")}>★</span>
                    {/if}
                  </span>
                  <span class="tile-price type-hub-numeric tabular-nums"
                    >{money(product.price_minor)}</span
                  >
                  {#if tag}
                    <span class="tag type-hub-label" data-testid="pos-tag">{t(tag)}</span>
                  {/if}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>
  {/if}

  {#if !viewport.phone}
    <aside class="side">{@render cartPanel()}</aside>
  {:else if sheet}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="cart-sheet"
      role="dialog"
      aria-modal="true"
      aria-labelledby="sheet-title"
      tabindex="-1"
      data-testid="cart-sheet"
      onkeydown={onSheetKey}
    >
      <div class="sheet-head">
        <h2 class="type-hub-title" id="sheet-title" tabindex="-1" bind:this={sheetHeading}>
          {t("hub.pos.sheet.title")}
        </h2>
        <Button label="hub.pos.sheet.close" variant="secondary" onclick={() => void closeCart()} />
      </div>
      {@render cartPanel()}
    </div>
  {:else if lines.length > 0}
    <button
      type="button"
      class="ui-interactive phone-cart-bar type-hub-title tabular-nums"
      data-component="phone-cart-bar"
      data-testid="phone-cart-bar"
      bind:this={barButton}
      onclick={() => void openCart()}
      >{count === 1
        ? t("hub.pos.bar.one", { total: money(total) })
        : t("hub.pos.bar.many", { count, total: money(total) })}</button
    >
  {/if}
</section>

<style>
  .pos {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(18rem, 26rem);
    column-gap: var(--space-7);
    block-size: 100%;
    min-block-size: 0;
  }

  .pos.phone {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr) auto;
    row-gap: var(--space-3);
  }

  h1,
  h2,
  h3,
  p {
    margin: 0;
  }

  .catalogue {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-inline-size: 0;
    min-block-size: 0;
  }

  .catalogue > :not(.results) {
    flex: none;
  }

  /* The heading, strip and search stay put; the tiles scroll. */
  .results {
    flex: 1 1 auto;
    min-block-size: 0;
    overflow-y: auto;
    padding: var(--space-1);
  }

  .strip {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .chip,
  .link,
  .step {
    --ui-bg: var(--button-secondary-background);
    min-block-size: var(--space-touch-min);
    min-inline-size: var(--space-touch-min);
    padding: 0 var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    color: var(--button-secondary-foreground);
    font-weight: var(--type-hub-title-weight);
    cursor: pointer;
  }

  .chip[aria-pressed="true"] {
    border-width: 2px;
    border-color: var(--accent);
  }

  .search {
    display: grid;
    gap: var(--space-1);
    max-inline-size: 28rem;
  }

  .control {
    min-block-size: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface);
    color: var(--text);
    font: inherit;
  }

  .notice {
    min-block-size: 1lh;
    color: var(--text-muted);
  }

  .notice.refusal,
  .refusal {
    color: var(--warning);
  }

  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(10rem, 1fr));
    gap: var(--space-4);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .tile {
    display: grid;
    grid-template-rows: auto 1fr auto auto;
    gap: var(--space-2);
    inline-size: 100%;
    block-size: 100%;
    box-sizing: border-box;
    padding: var(--space-hub-tile-padding);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--hub-tile);
    color: var(--text);
    text-align: start;
    cursor: pointer;
  }

  .tile:disabled {
    color: var(--text);
    cursor: not-allowed;
  }

  .photo {
    display: grid;
    place-items: center;
    aspect-ratio: 4 / 3;
    overflow: hidden;
    border-radius: var(--radius-sm, 8px);
    background-color: var(--surface);
  }

  .photo img {
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
  }

  .photo.dim {
    opacity: 0.4;
  }

  .tile-price {
    color: var(--text);
  }

  .tag {
    justify-self: start;
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    color: var(--text);
  }

  .failed {
    display: grid;
    justify-items: start;
    gap: var(--space-3);
  }

  .side {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-block-size: 0;
    max-block-size: 100%;
  }

  .cart {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: var(--space-3);
    min-block-size: 0;
    box-sizing: border-box;
    padding: var(--space-hub-panel-padding);
    border: var(--cart-border);
    border-radius: var(--radius-xl);
    background-color: var(--cart-hub-background);
    color: var(--cart-foreground);
    overflow-y: auto;
  }

  .cart-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .who {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .chooser {
    display: grid;
    gap: var(--space-3);
  }

  .empty {
    color: var(--text-muted);
  }

  .lines,
  .parts {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .line {
    display: grid;
    gap: var(--space-2);
    padding-block-end: var(--space-3);
    border-block-end: 1px solid var(--line);
  }

  .line-main {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .stepper {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .step {
    padding: 0;
    font-size: 1.25rem;
  }

  .qty {
    min-inline-size: 2ch;
    text-align: center;
  }

  .parts {
    gap: var(--space-1);
    padding-inline-start: var(--space-5);
    color: var(--text-muted);
  }

  .settle {
    display: grid;
    gap: var(--space-3);
    box-sizing: border-box;
    padding: var(--space-4) var(--space-hub-panel-padding);
    border-block-start: var(--settle-bar-border-top);
    background-color: var(--settle-bar-background);
    color: var(--text);
  }

  :global([data-theme="hc"]) .settle {
    border-block-start: var(--settle-bar-border-top-hc);
    background-color: var(--settle-bar-background-hc);
  }

  .total {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .phone-cart-bar {
    min-block-size: var(--space-9);
    inline-size: 100%;
    border: 0;
    border-block-start: var(--phone-cart-bar-border-top);
    background-color: var(--phone-cart-bar-background);
    color: var(--text);
    cursor: pointer;
  }

  :global([data-theme="hc"]) .phone-cart-bar {
    border-block-start: var(--phone-cart-bar-border-top-hc);
  }

  .cart-sheet {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-block-size: 0;
    block-size: 100%;
  }

  .cart-sheet .cart {
    flex: 1 1 auto;
  }

  .sheet-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .sheet-head h2:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }
</style>
