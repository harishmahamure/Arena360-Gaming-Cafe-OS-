<script lang="ts">
  import { formatMoney, t } from "@arena/ui";
  import type { ShopItem, SnapshotLocale } from "@arena/ui/generated";

  let {
    item,
    image = null,
    locale,
    onadd = () => {},
  }: {
    item: ShopItem;
    /** The photo, a JPEG data URL from the Hub; `null` while it loads or when there is none. */
    image?: string | null;
    /** The branch's money format, once the attract view has told it. */
    locale: SnapshotLocale | null;
    /** Add was activated (the cart arrives with Story 6.31). */
    onadd?: (item: ShopItem) => void;
  } = $props();

  let broken: string | null = $state(null);
  const photo = $derived(image !== null && image !== broken ? image : null);
  const price = $derived(locale ? formatMoney(item.price_minor, locale) : "");
</script>

<!-- A `shop-item-card`: the photo with the popular badge on its top-left, the name, the tabular
     price and Add. Out of stock dims the photo only: the name and the tag stay at full ink. -->
<article
  class="shop-item-card"
  class:unavailable={!item.available}
  data-testid="shop-item-card"
  data-available={item.available ? "true" : "false"}
  data-item-id={item.id}
>
  <div class="photo-wrap">
    {#if item.popular}
      <span class="popular-badge type-kiosk-eyebrow" data-testid="popular-badge">
        {t("player.shop.popular")}
      </span>
    {/if}
    <div class="photo" data-testid="shop-item-photo">
      {#if photo !== null}
        <img src={photo} alt="" draggable="false" onerror={() => (broken = photo)} />
      {/if}
    </div>
  </div>
  <div class="body">
    <h3 class="name type-kiosk-title" data-testid="shop-item-name">{item.name}</h3>
    {#if item.components.length > 0}
      <ul
        class="components type-kiosk-body"
        aria-label={t("player.shop.components_label")}
        data-testid="shop-item-components"
      >
        {#each item.components as part, index (index)}
          <li>{t("player.shop.component", { qty: part.qty, name: part.name })}</li>
        {/each}
      </ul>
    {/if}
    <p class="price type-kiosk-title tabular-nums" data-testid="shop-item-price">{price}</p>
    {#if !item.available}
      <p class="oos type-kiosk-label" data-testid="shop-item-oos">
        {t("player.shop.out_of_stock")}
      </p>
    {/if}
    <button
      type="button"
      class="add type-kiosk-label"
      disabled={!item.available}
      aria-label={t("player.shop.add_label", { name: item.name })}
      data-testid="shop-item-add"
      onclick={() => onadd(item)}
    >
      {t("player.shop.add")}
    </button>
  </div>
</article>

<style>
  .shop-item-card {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: var(--shop-item-card-border);
    border-radius: var(--radius-lg);
    background-color: var(--shop-item-card-background);
    color: var(--text);
    text-align: left;
  }

  .photo-wrap {
    position: relative;
  }

  .photo {
    height: 170px;
    background-color: var(--bg);
  }

  .photo img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    user-select: none;
  }

  /* Out of stock: only the photo dims. */
  .unavailable .photo {
    opacity: 0.35;
  }

  .popular-badge {
    position: absolute;
    z-index: 1;
    top: var(--space-3);
    left: var(--space-3);
    padding: var(--space-1) var(--space-3);
    border: var(--popular-badge-border);
    border-radius: var(--radius-full);
    background-color: var(--popular-badge-background);
    color: var(--popular-badge-foreground);
    text-transform: uppercase;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-4) var(--space-5) var(--space-5);
  }

  .body > :global(*) {
    margin: 0;
  }

  .name {
    overflow-wrap: anywhere;
  }

  .components {
    margin: 0;
    padding: 0;
    color: var(--text-muted);
    list-style: none;
  }

  .oos {
    align-self: flex-start;
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    display: inline-flex;
    align-items: center;
    margin-top: var(--space-2);
    padding: 0 var(--space-4);
    border: 1px dashed var(--border-strong);
    border-radius: var(--radius-full);
    color: var(--text);
  }

  .add {
    align-self: flex-start;
    box-sizing: border-box;
    min-height: var(--space-touch-min);
    margin-top: var(--space-2);
    padding: 0 var(--space-5);
    border: var(--button-secondary-border);
    border-radius: var(--radius-full);
    background-color: var(--button-secondary-background);
    color: var(--button-secondary-foreground);
    cursor: pointer;
  }

  .add:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  .add:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: 3px;
  }
</style>
