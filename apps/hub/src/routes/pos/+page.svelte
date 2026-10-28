<script lang="ts">
  import { Button, InlinePin, formatMoney, formatTime, t } from "@arena/ui";
  import type {
    CreditView,
    IntentPayload,
    PaymentMethod,
    PaymentRow,
    PlayerHit,
    PosSaleView,
    Product,
    SnapshotBill,
  } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    dropElevation,
    elevate,
    hasPermission,
    loadSettings,
    loadWallet,
    newIdempotencyKey,
    submitIntent,
  } from "../../lib/auth";
  import { pushBack } from "../../lib/back";
  import { catalog } from "../../lib/catalog.svelte";
  import { branchLocale } from "../../lib/locale.svelte";
  import { isTextEntry } from "../../lib/nav";
  import { sessionStaff, snapshotOf } from "../../lib/operating";
  import PlayerLookup from "../../lib/players/PlayerLookup.svelte";
  import {
    ALL,
    DEFAULT_TAX,
    FAVOURITES,
    ScanBurst,
    cartBill,
    cartCount,
    categoriesOf,
    findByBarcode,
    lineShortage,
    saleLines,
    step,
    unavailableTag,
    visibleProducts,
    type CartLine,
    type Refusal,
  } from "../../lib/pos";
  import { loadPosSales } from "../../lib/pos-sales";
  import { cartAmount, orderSessions } from "../../lib/pos-session";
  import { loadProducts, priceText } from "../../lib/products";
  import SettleBar from "../../lib/SettleBar.svelte";
  import { KEY_TAX, parseTax, type Tax } from "../../lib/tax";
  import UndoDock from "../../lib/UndoDock.svelte";
  import { offerUndo, posRestore } from "../../lib/undo.svelte";
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

  // The session the cart is attached to (Story 5.12): `?session=<id>` from a tile's Order, or one
  // picked with Change. The cart then adds to that session's bill instead of settling.
  let attachedId = $state<string | null>(page.url.searchParams.get("session"));

  // Settling (Story 5.11).
  let tax: Tax = $state(DEFAULT_TAX);
  let walletMinor: number | null = $state(null);
  let credit: CreditView | null = $state(null);
  let busy = $state(false);
  let error: MessageKey | null = $state(null);
  let shortfall: { message: string; payments: PaymentRow[] } | null = $state(null);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let attempt: { key: string; body: string } | null = null;

  // Recent sales.
  let view: "grid" | "recent" = $state("grid");
  let sales: PosSaleView[] = $state([]);
  let salesState: "loading" | "ok" | "failed" = $state("loading");
  let openSale: string | null = $state(null);
  let salesLoads = 0;

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
  const bill = $derived(cartBill(cart, byId, tax));
  const total = $derived(bill.total_minor);
  const staff = $derived(sessionStaff(page.data));
  const locale = $derived(branchLocale.current);
  const money = (minor: number) => (locale ? formatMoney(minor, locale) : priceText(minor, 2));
  // What the settle bar shows: the server's figure, ported (the wallet and credit rows appear
  // once a player is attached).
  const settleBill = $derived<SnapshotBill>({
    session_id: "",
    device_id: "",
    lines: [],
    subtotal_minor: bill.subtotal_minor,
    paid_minor: 0,
    remaining_minor: total,
    wallet_minor: attached ? walletMinor : null,
    credit: attached ? credit : null,
  });
  const snapshot = $derived(snapshotOf(page.data));
  const running = $derived(orderSessions(snapshot?.sessions ?? [], snapshot?.devices ?? []));
  const attachedSession = $derived(running.find((s) => s.session.id === attachedId) ?? null);
  // What an order adds to the session bill: the lines at their prices, no tax.
  const placedAmount = $derived(cartAmount(cart, byId));
  const sheet = $derived(viewport.phone && cartOpen);
  const sessionHeader = $derived(
    attachedSession
      ? t("hub.pos.cart.in_session", {
          player:
            attachedSession.player ??
            t("hub.session.guest", { n: attachedSession.session.guest_no ?? 0 }),
          device: attachedSession.device,
        })
      : null,
  );
  const who = $derived(attached ? attached.name || attached.username || attached.phone : null);

  const METHOD_NAME: Record<PaymentMethod, MessageKey> = {
    cash: "hub.settle.method.cash",
    upi: "hub.settle.method.upi",
    card: "hub.settle.method.card",
    wallet: "hub.settle.method.wallet",
    credit: "hub.settle.method.credit",
  };

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
    const kept = cart.filter((l) => known.has(l.productId));
    // Only a cart that really lost a line is a new cart (a refusal about it would be stale).
    if (kept.length !== cart.length) cart = kept;
  }

  async function loadTax() {
    const outcome = await loadSettings();
    if (outcome.kind !== "ok") return;
    const row = outcome.settings.find((s) => s.key === KEY_TAX);
    const parsed = row ? parseTax(row.value) : null;
    if (parsed) tax = parsed;
  }

  async function loadSales() {
    const mine = ++salesLoads;
    const outcome = await loadPosSales();
    if (mine !== salesLoads) return;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    salesState = outcome.kind === "ok" ? "ok" : "failed";
    if (outcome.kind === "ok") sales = outcome.view.sales;
  }

  onMount(() => {
    void load();
    void loadTax();
    void loadSales();
  });
  $effect(() => {
    if (catalog.version) {
      untrack(() => {
        void load();
        void loadSales();
      });
    }
  });

  // A tile's Order opens the POS on its session.
  $effect(() => {
    const id = page.url.searchParams.get("session");
    untrack(() => {
      if (id) attachedId = id;
    });
  });

  // The attached player's wallet and credit join the payment methods.
  $effect(() => {
    const id = attached?.player_id ?? null;
    untrack(() => void loadPayer(id));
  });

  async function loadPayer(id: string | null) {
    walletMinor = null;
    credit = null;
    if (id === null) return;
    const outcome = await loadWallet(id);
    if (attached?.player_id !== id) return;
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind !== "ok") return;
    walletMinor = outcome.value.balance_minor;
    credit = outcome.value.credit ?? null;
  }

  // A cart that changed is no longer what a refused sale was about.
  $effect(() => {
    void cart;
    untrack(() => {
      shortfall = null;
      error = null;
    });
  });

  // An undone sale gives its cart (and player) back.
  $effect(() => {
    const restored = posRestore.current;
    if (!restored) return;
    untrack(() => {
      posRestore.current = null;
      // A cart already started for the next customer is never overwritten.
      if (cart.length === 0) {
        cart = restored.cart;
        attached = restored.player;
        if (restored.session) attachedId = restored.session;
      }
      refusals = {};
      notice = null;
      void load();
      void loadSales();
    });
  });

  function refusalText({ name, left, cap }: Refusal): string {
    return cap ? t("hub.pos.cart.cap", { name, left }) : t("hub.pos.cart.limit", { name, left });
  }

  /** Puts one of `product` in the cart (a tile, a scan); a refusal goes in the notice. */
  function add(product: Product): boolean {
    const tag = unavailableTag(product);
    if (tag) {
      notice = {
        tone: "refusal",
        text: t("hub.pos.cannot_add", { name: product.name, reason: t(tag).toLowerCase() }),
      };
      return false;
    }
    const moved = step(cart, product, 1, byId);
    if (moved.refusal) {
      notice = { tone: "refusal", text: refusalText(moved.refusal) };
      return false;
    }
    cart = moved.cart;
    refusals = {};
    notice = { tone: "ok", text: t("hub.pos.added", { name: product.name }) };
    return true;
  }

  /** The ± steppers: a step up past what is on hand is refused under that line. */
  function nudge(product: Product, delta: number) {
    const tag = delta > 0 ? unavailableTag(product) : null;
    if (tag) {
      refusals = {
        [product.id]: t("hub.pos.cannot_add", {
          name: product.name,
          reason: t(tag).toLowerCase(),
        }),
      };
      return;
    }
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

  /** Adds the product with this exact barcode; false when nothing was added. */
  function scan(code: string): boolean {
    const wanted = code.trim();
    if (!wanted) return false;
    const product = findByBarcode(products, wanted);
    if (!product) {
      notice = { tone: "refusal", text: t("hub.pos.barcode_unknown", { code: wanted }) };
      return false;
    }
    return add(product);
  }

  function onSearchKey(event: KeyboardEvent) {
    if (event.key !== "Enter") return;
    event.preventDefault();
    if (event.isComposing || !query.trim()) return;
    // A code that added nothing stays in the field so a mistyped digit can be fixed.
    if (scan(query)) query = "";
  }

  /** A keyboard-wedge scan while focus is on the POS outside a text field. */
  function onWindowKey(event: KeyboardEvent) {
    if (event.ctrlKey || event.altKey || event.metaKey || event.defaultPrevented) return;
    // Behind the phone sheet the notice is not on screen, so a scan there would fail silently.
    if (sheet || event.repeat || isTextEntry(document.activeElement)) return burst.reset();
    const code = burst.key(event.key, event.timeStamp);
    if (code === null) return;
    event.preventDefault();
    scan(code);
  }

  const REFUSAL: Record<string, MessageKey> = {
    PAYMENT_SHORT: "hub.settle.short",
    INSUFFICIENT_BALANCE: "hub.settle.insufficient",
    CREDIT_LIMIT: "hub.settle.credit_limit",
    CREDIT_PAUSED: "hub.plans.sell.credit_paused",
    REFERENCE_REQUIRED: "hub.settle.reference",
    OUT_OF_STOCK: "hub.pos.err.out_of_stock",
    SHIFT_NOT_OPEN: "hub.settle.shift_closed",
    PERMISSION_DENIED: "hub.refusal.permission",
    NOT_FOUND: "hub.pos.err.player",
    VALIDATION_FAILED: "hub.pos.err.lines",
  };

  /** Adds the cart to the attached session's bill. `over` orders a short bundle anyway. */
  async function place(over = false, ticket?: string) {
    const target = attachedSession;
    if (busy || lines.length === 0 || !target) return;
    busy = true;
    error = null;
    const payload: IntentPayload = {
      kind: "order.place",
      session_id: target.session.id,
      lines: saleLines(cart),
      stock_override: over ? true : undefined,
    };
    const body = JSON.stringify(payload);
    const key = attempt?.body === body ? attempt.key : newIdempotencyKey();
    attempt = { key, body };
    const amount = placedAmount;
    const ordered = {
      cart: cart.map((l) => ({ ...l })),
      player: null,
      session: target.session.id,
    };
    const outcome = await submitIntent(payload, key, undefined, {
      elevationTicket: ticket,
    }).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      attempt = null;
      shortfall = null;
      offerUndo(outcome.result, "pos", "end", ordered);
      cart = [];
      refusals = {};
      cartOpen = false;
      notice = {
        tone: "ok",
        text: t("hub.pos.session.placed", { amount: money(amount) }),
      };
      void load();
      return;
    }
    if (ticket) void dropElevation(ticket);
    if (outcome.kind !== "error") {
      error = "hub.pos.err.unreachable";
      return;
    }
    const { code, message, field } = outcome.error;
    if (code === "BUNDLE_SHORT") {
      approvalPrompt = "hub.pos.override.approves_order";
      shortfall = { message, payments: [] };
      return;
    }
    if (code === "PIN_REQUIRED" && over) {
      approvalPrompt = "ui.pin.again";
      shortfall = { message: shortfall?.message ?? message, payments: [] };
      return;
    }
    error =
      code === "VALIDATION_FAILED" && field === "session_id"
        ? "hub.pos.err.session"
        : code === "NOT_FOUND"
          ? "hub.pos.err.session"
          : code === "VALIDATION_FAILED"
            ? "hub.pos.err.lines"
            : code === "PERMISSION_DENIED"
              ? "hub.refusal.permission"
              : code === "OUT_OF_STOCK"
                ? "hub.pos.err.out_of_stock"
                : "hub.pos.err.place";
    if (code === "OUT_OF_STOCK") void load();
  }

  /** Carries out the refused cart again with the stock override. */
  function sellAnyway(ticket?: string) {
    const pending = shortfall;
    if (!pending) return;
    if (attachedSession) void place(true, ticket);
    else void settle(pending.payments, true, ticket);
  }

  /** Settles the cart. `over` sells a short bundle anyway (needs `stock.override`). */
  async function settle(payments: PaymentRow[], over = false, ticket?: string) {
    if (busy || lines.length === 0) return;
    busy = true;
    error = null;
    const payload: IntentPayload = {
      kind: "pos.sale",
      lines: saleLines(cart),
      payments,
      player_id: attached?.player_id,
      stock_override: over ? true : undefined,
    };
    const body = JSON.stringify(payload);
    const key = attempt?.body === body ? attempt.key : newIdempotencyKey();
    attempt = { key, body };
    const sold = { cart: cart.map((l) => ({ ...l })), player: attached };
    const amount = total;
    const outcome = await submitIntent(payload, key, undefined, {
      elevationTicket: ticket,
    }).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      attempt = null;
      shortfall = null;
      offerUndo(outcome.result, "pos", "end", sold);
      cart = [];
      attached = null;
      refusals = {};
      cartOpen = false;
      notice = { tone: "ok", text: t("hub.pos.sold", { amount: money(amount) }) };
      void load();
      void loadSales();
      return;
    }
    if (ticket) void dropElevation(ticket);
    if (outcome.kind !== "error") {
      error = "hub.pos.err.unreachable";
      return;
    }
    const { code, message } = outcome.error;
    if (code === "BUNDLE_SHORT") {
      approvalPrompt = "hub.pos.override.approves";
      shortfall = { message, payments };
      return;
    }
    if (code === "PIN_REQUIRED" && over) {
      approvalPrompt = "ui.pin.again";
      shortfall = { message: shortfall?.message ?? message, payments };
      return;
    }
    error = REFUSAL[code] ?? "hub.pos.err.failed";
    if (code === "OUT_OF_STOCK") void load();
  }

  /** The manager's PIN for selling a short bundle: one approval, one sale. */
  async function approve(pin: string) {
    const pending = shortfall;
    if (!pending) return { ok: false, message: "ui.pin.failed" } as const;
    const outcome = await elevate(
      [{ intent_kind: attachedSession ? "order.place" : "pos.sale" }],
      pin,
    );
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    sellAnyway(outcome.ticket);
    return { ok: true } as const;
  }

  /** A looked-up player attaches their running session, or (without one) the sale itself. */
  async function pick(hit: PlayerHit | null) {
    const theirs = hit ? running.find((s) => s.session.player_id === hit.player_id) : undefined;
    attachedId = theirs?.session.id ?? null;
    attached = theirs ? null : hit;
    choosing = false;
    await tick();
    changeButton?.focus();
  }

  /** Attaches the cart to a running session picked by its PC. */
  async function pickSession(id: string) {
    attachedId = id;
    attached = null;
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

<UndoDock {staff} />

{#snippet cartPanel()}
  <section class="cart" data-component="cart" data-testid="cart" aria-labelledby="cart-title">
    <div class="cart-head">
      <h2 class="type-hub-title" id="cart-title">{t("hub.pos.cart.title")}</h2>
      <p class="who type-hub-label">
        <span data-testid="cart-for" data-session-id={attachedSession?.session.id}
          >{sessionHeader ?? who ?? t("hub.pos.cart.walkup")}</span
        >
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
        {#if running.length > 0}
          <div class="running" role="group" aria-label={t("hub.pos.session.running")}>
            <p class="type-hub-label">{t("hub.pos.session.running")}</p>
            <ul class="sessions" data-testid="cart-sessions">
              {#each running as s (s.session.id)}
                <li>
                  <button
                    type="button"
                    class="ui-interactive link type-hub-body"
                    data-testid="cart-session-option"
                    data-session-id={s.session.id}
                    aria-pressed={s.session.id === attachedSession?.session.id}
                    onclick={() => void pickSession(s.session.id)}
                    >{t("hub.pos.session.pick", {
                      player: s.player ?? t("hub.session.guest", { n: s.session.guest_no ?? 0 }),
                      device: s.device,
                    })}</button
                  >
                </li>
              {/each}
            </ul>
          </div>
        {/if}
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
      {#if attachedSession}
        <p class="type-hub-label tabular-nums" data-testid="cart-placed-total">
          {t("hub.pos.session.total", { amount: money(placedAmount) })}
        </p>
      {:else if bill.tax_minor > 0}
        <dl class="breakup type-hub-label tabular-nums" data-testid="pos-tax">
          <div>
            <dt>{t("hub.pos.subtotal")}</dt>
            <dd data-testid="pos-subtotal">{money(bill.subtotal_minor)}</dd>
          </div>
          {#each bill.breakup.filter((r) => r.tax_minor > 0) as r (r.rate_bp)}
            <div data-testid="pos-tax-rate">
              <dt>{t("hub.pos.tax_line", { label: tax.label, rate: r.rate_bp / 100 })}</dt>
              <dd>{money(r.tax_minor)}</dd>
            </div>
          {/each}
        </dl>
      {/if}
    {/if}
  </section>

  {#if attachedSession}
    <section
      class="settle"
      data-component="settle-bar"
      data-testid="order-bar"
      aria-labelledby="pos-order-title"
    >
      <h3 class="type-hub-title" id="pos-order-title">{t("hub.pos.session.bar")}</h3>
      <Button
        label="hub.pos.session.add"
        disabled={busy || lines.length === 0}
        disabledReason={busy ? "hub.sheet.waiting" : "hub.pos.session.add_empty"}
        onclick={() => void place()}
      />
    </section>
  {:else if lines.length > 0 && locale}
    <SettleBar
      bill={settleBill}
      {locale}
      {busy}
      onsettle={(payments) => void settle(payments)}
      title="hub.pos.settle.title"
      prefillCash
      hotkey
    />
  {:else}
    <section
      class="settle"
      data-component="settle-bar"
      data-testid="settle-bar"
      aria-labelledby="pos-settle-title"
    >
      <h3 class="type-hub-title" id="pos-settle-title">{t("hub.pos.settle.title")}</h3>
      <Button label="hub.pos.settle.submit" disabled={true} disabledReason="hub.pos.settle.empty" />
    </section>
  {/if}

  {#if error}
    <p class="refusal type-hub-label" role="alert" data-testid="pos-error">
      <span aria-hidden="true">⚠</span>
      {t(error)}
    </p>
  {/if}

  {#if shortfall}
    <div class="override" role="alert" data-testid="pos-override">
      <p class="refusal type-hub-label">
        <span aria-hidden="true">⚠</span>
        {t("hub.pos.short.title")}
        <span data-testid="pos-short-message">{shortfall.message}</span>
      </p>
      {#if hasPermission(staff, "stock.override")}
        <div class="override-actions">
          <Button
            label={attachedSession ? "hub.pos.override.order" : "hub.pos.override.sell"}
            disabled={busy}
            disabledReason="hub.sheet.waiting"
            onclick={() => sellAnyway()}
          />
          <Button
            label="hub.pos.override.cancel"
            variant="secondary"
            onclick={() => (shortfall = null)}
          />
        </div>
      {:else}
        <InlinePin
          message={approvalPrompt}
          onsubmit={approve}
          oncancel={() => (shortfall = null)}
        />
      {/if}
    </div>
  {/if}
{/snippet}

{#snippet recentSales()}
  <section class="recent" data-testid="recent-sales" aria-labelledby="recent-title">
    <h2 class="type-hub-title" id="recent-title">{t("hub.pos.recent.title")}</h2>
    {#if salesState === "loading"}
      <p class="type-hub-body" role="status">{t("hub.pos.recent.loading")}</p>
    {:else if salesState === "failed" && sales.length === 0}
      <div class="failed" role="alert">
        <p class="type-hub-body">{t("hub.pos.recent.load_failed")}</p>
        <Button label="hub.pos.retry" variant="secondary" onclick={() => void loadSales()} />
      </div>
    {:else if sales.length === 0}
      <p class="type-hub-body" data-testid="recent-empty">{t("hub.pos.recent.empty")}</p>
    {:else}
      <ul class="sales" aria-label={t("hub.pos.recent.list")}>
        {#each sales as sale (sale.order_id)}
          {@const opened = openSale === sale.order_id}
          <li class="sale" data-testid="recent-sale" data-order-id={sale.order_id}>
            <div class="sale-row">
              <span class="type-hub-label tabular-nums"
                >{formatTime(sale.settled_at, locale?.tz)}</span
              >
              <span class="type-hub-body"
                >{sale.player ?? t("hub.pos.cart.walkup")} · {t("hub.pos.recent.by", {
                  name: sale.staff,
                })}</span
              >
              <span class="type-hub-label" data-testid="recent-methods"
                >{[
                  ...new Set(sale.payments.map((p) => t(METHOD_NAME[p.method as PaymentMethod]))),
                ].join(", ")}</span
              >
              <span class="type-hub-numeric tabular-nums" data-testid="recent-total"
                >{money(sale.total_minor)}</span
              >
              <span class="type-hub-label" data-testid="recent-invoice"
                >{sale.invoice_no
                  ? t("hub.pos.recent.invoice", { number: sale.invoice_no })
                  : t("hub.pos.recent.invoice_pending")}</span
              >
              <button
                type="button"
                class="ui-interactive link type-hub-label"
                data-testid="recent-toggle-detail"
                aria-expanded={opened}
                aria-controls="sale-{sale.order_id}"
                onclick={() => (openSale = opened ? null : sale.order_id)}
                >{opened ? t("hub.pos.recent.hide") : t("hub.pos.recent.details")}</button
              >
            </div>
            {#if opened}
              <div class="detail" id="sale-{sale.order_id}" data-testid="sale-detail">
                <ul class="detail-lines type-hub-body">
                  {#each sale.lines as l, i (i)}
                    <li data-testid="sale-detail-line">
                      <span
                        >{t("hub.pos.recent.line", {
                          qty: l.qty,
                          name: l.name,
                          price: money(l.unit_price_minor),
                        })}</span
                      >
                      <span class="tabular-nums">{money(l.amount_minor)}</span>
                    </li>
                  {/each}
                </ul>
                {#if sale.tax_minor > 0}
                  <p class="type-hub-label tabular-nums">
                    {t("hub.pos.subtotal")}: {money(sale.subtotal_minor)} · {tax.label}: {money(
                      sale.tax_minor,
                    )}
                  </p>
                {/if}
                <p class="type-hub-label tabular-nums">
                  {t("hub.pos.recent.total")}: {money(sale.total_minor)}
                </p>
                <ul class="detail-lines type-hub-label">
                  {#each sale.payments as p, i (i)}
                    <li data-testid="sale-detail-payment">
                      {t("hub.pos.recent.paid", {
                        method: t(METHOD_NAME[p.method as PaymentMethod]),
                      })}
                      <span class="tabular-nums">{money(p.amount_minor)}</span>
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/snippet}

<section class="pos" class:phone={viewport.phone} data-testid="pos">
  {#if !sheet}
    <div class="catalogue" data-component="pos-product-grid" data-testid="pos-product-grid">
      <div class="head">
        <h1 class="type-hub-numeric">{t("hub.nav.pos")}</h1>
        <button
          type="button"
          class="ui-interactive link type-hub-label"
          data-testid="recent-toggle"
          aria-pressed={view === "recent"}
          onclick={() => {
            view = view === "recent" ? "grid" : "recent";
            if (view === "recent") void loadSales();
          }}>{view === "recent" ? t("hub.pos.recent.back") : t("hub.pos.recent.toggle")}</button
        >
      </div>

      {#if view === "recent"}
        <div class="results">{@render recentSales()}</div>
      {:else}
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
      {/if}
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

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .recent {
    display: grid;
    gap: var(--space-3);
  }

  .sales,
  .detail-lines {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .sale {
    display: grid;
    gap: var(--space-2);
    padding-block-end: var(--space-3);
    border-block-end: 1px solid var(--line);
  }

  .sale-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
  }

  .detail {
    display: grid;
    gap: var(--space-2);
    padding-inline-start: var(--space-4);
  }

  .detail-lines li {
    display: flex;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .breakup {
    display: grid;
    gap: var(--space-1);
    margin: 0;
    color: var(--text-muted);
  }

  .breakup div {
    display: flex;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .breakup dt,
  .breakup dd {
    margin: 0;
  }

  .override {
    display: grid;
    gap: var(--space-3);
  }

  .override-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
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

  .tile.ui-interactive:disabled {
    color: var(--text);
    cursor: not-allowed;
    /* Only the photo dims; the name, price and tag stay at full ink. */
    opacity: 1;
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

  .chooser,
  .running {
    display: grid;
    gap: var(--space-3);
  }

  .running p {
    margin: 0;
  }

  .sessions {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
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
