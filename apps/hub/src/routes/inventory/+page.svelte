<script lang="ts">
  import { Button, InlinePin, TextField, t } from "@arena/ui";
  import type { IntentPayload, Product } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { onMount, tick, untrack } from "svelte";
  import { invalidate } from "$app/navigation";
  import { page } from "$app/state";
  import {
    SESSION_DEPENDENCY,
    elevate,
    hasPermission,
    loadSettings,
    newIdempotencyKey,
    submitIntent,
  } from "../../lib/auth";
  import { catalog } from "../../lib/catalog.svelte";
  import { branchLocale } from "../../lib/locale.svelte";
  import { sessionStaff } from "../../lib/operating";
  import { loadProducts, parsePrice, priceText, shrinkImage } from "../../lib/products";
  import { KEY_TAX, parsePct, parseTax, pct, type Tax } from "../../lib/tax";

  type Job = { payload: IntentPayload; rev?: number; key: string };

  const INVALID: Record<string, MessageKey> = {
    name: "hub.products.invalid.name",
    category: "hub.products.invalid.category",
    price_minor: "hub.products.invalid.price",
    tax_rate_bp: "hub.products.invalid.tax",
    hsn: "hub.products.invalid.hsn",
    barcode: "hub.products.invalid.barcode",
    image: "hub.products.invalid.image",
    components: "hub.products.invalid.components",
    bundle: "hub.products.invalid.bundle",
  };
  const BLANK = {
    name: "",
    category: "",
    price: "",
    tax: "",
    hsn: "",
    barcode: "",
    image: "",
    track: true,
    kitchen: false,
    favourite: false,
    active: true,
    bundle: false,
    parts: [] as { id: string; qty: string }[],
  };

  let products: Product[] = $state([]);
  let tax = $state<Tax | null>(null);
  let loading = $state(true);
  let loadFailed = $state(false);
  let editing: Product | null = $state(null);
  let form = $state({ ...BLANK });
  let filter = $state("");
  let errors: Record<string, MessageKey> = $state({});
  let busy = $state(false);
  let approving: Job | null = $state(null);
  let approvalPrompt: MessageKey = $state("ui.pin.prompt");
  let status: { tone: "ok" | "refusal"; message: MessageKey } | null = $state(null);
  let formEl: HTMLFormElement | undefined = $state();

  const staff = $derived(sessionStaff(page.data));
  const needsApproval = $derived(!hasPermission(staff, "catalog.edit"));
  const exponent = $derived(branchLocale.current?.exponent ?? 2);
  const symbol = $derived(branchLocale.current?.symbol ?? "");
  const gst = $derived(tax?.country === "IN" && tax.gst);
  const defaultRate = $derived(tax ? (gst ? tax.product_bp : tax.rate_bp) : 0);
  const categories = $derived([...new Set(products.map((p) => p.category).filter(Boolean))]);
  const pickable = $derived(products.filter((p) => !p.bundle && p.id !== editing?.id));
  const options = $derived([
    ...pickable.map((p) => ({ id: p.id, name: p.name })),
    ...((editing as Product | null)?.components ?? [])
      .filter((c) => !pickable.some((p) => p.id === c.product_id))
      .map((c) => ({ id: c.product_id, name: c.name })),
  ]);
  const shown = $derived(products.filter((p) => !filter || p.category === filter));

  let loads = 0;
  let idem: { json: string; key: string } | null = null;

  function keyFor(payload: IntentPayload) {
    const json = JSON.stringify(payload);
    if (idem?.json !== json) idem = { json, key: newIdempotencyKey() };
    return idem.key;
  }

  function deletion(p: Product): Job {
    const payload: IntentPayload = { kind: "product.delete", id: p.id };
    return { payload, rev: p.rev, key: keyFor(payload) };
  }

  async function load() {
    const mine = ++loads;
    const [list, settings] = await Promise.all([loadProducts(), loadSettings()]);
    if (mine !== loads) return;
    loading = false;
    if (list.kind === "signed_out" || settings.kind === "signed_out") {
      return void (await invalidate(SESSION_DEPENDENCY));
    }
    loadFailed = list.kind !== "ok";
    if (list.kind === "ok") products = list.products;
    if (settings.kind === "ok") {
      const row = settings.settings.find((s) => s.key === KEY_TAX);
      tax = row ? parseTax(row.value) : null;
    }
  }

  onMount(() => void load());
  $effect(() => {
    if (catalog.version) untrack(() => void load());
  });
  $effect(() => {
    if (filter && !categories.includes(filter)) filter = "";
  });

  function reset(product: Product | null = null) {
    editing = product;
    errors = {};
    form = product
      ? {
          ...BLANK,
          name: product.name,
          category: product.category,
          price: priceText(product.price_minor, exponent),
          tax: pct(product.tax_rate_bp),
          hsn: product.hsn,
          barcode: product.barcode,
          image: product.image,
          track: product.track_stock,
          kitchen: product.kitchen,
          favourite: product.favourite,
          active: product.active,
          bundle: product.bundle,
          parts: product.components.map((c) => ({ id: c.product_id, qty: String(c.qty) })),
        }
      : { ...BLANK, parts: [] };
    if (product) void tick().then(() => formEl?.querySelector("input")?.focus());
  }

  async function pick(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    const image = await shrinkImage(file);
    if (image === null) {
      form.image = "";
      errors = { ...errors, image: INVALID.image };
    } else {
      form.image = image;
      const { image: _, ...rest } = errors;
      errors = rest;
    }
  }

  async function run(job: Job, ticket?: string) {
    if (needsApproval && !ticket) {
      approvalPrompt = "ui.pin.prompt";
      approving = job;
      return;
    }
    if (busy) return;
    busy = true;
    status = null;
    errors = {};
    const outcome = await submitIntent(job.payload, job.key, job.rev, {
      elevationTicket: ticket,
    }).finally(() => (busy = false));
    if (outcome.kind === "signed_out") return void (await invalidate(SESSION_DEPENDENCY));
    if (outcome.kind === "applied") {
      idem = null;
      status = { tone: "ok", message: "hub.settings.saved" };
      const gone = job.payload.kind === "product.delete" && job.payload.id === editing?.id;
      if (job.payload.kind === "product.upsert" || gone) reset();
      return void (await load());
    }
    const err = outcome.kind === "error" ? outcome.error : null;
    if (err && err.code !== "PIN_REQUIRED") idem = null;
    if (err?.code === "PIN_REQUIRED") {
      approvalPrompt = "ui.pin.again";
      approving = job;
      return;
    }
    if (err?.code === "BARCODE_TAKEN")
      return void (errors = { barcode: "hub.products.barcode_taken" });
    if (err?.code === "HSN_REQUIRED") return void (errors = { hsn: INVALID.hsn });
    if (err?.code === "VALIDATION_FAILED" && err.field && INVALID[err.field]) {
      return void (errors = { [err.field]: INVALID[err.field] });
    }
    const stale =
      err?.code === "STALE" || (err?.code === "VALIDATION_FAILED" && err.field === "id");
    status = {
      tone: "refusal",
      message: stale
        ? "hub.products.stale"
        : err?.code === "PERMISSION_DENIED"
          ? "hub.refusal.permission"
          : "hub.settings.failed",
    };
    if (stale) {
      await load();
      if (editing) reset(products.find((p) => p.id === editing?.id) ?? null);
    }
  }

  async function approve(pin: string) {
    const job = approving;
    if (!job) return { ok: false, message: "ui.pin.failed" } as const;
    const outcome = await elevate([{ intent_kind: job.payload.kind }], pin);
    if (outcome.kind === "signed_out") {
      await invalidate(SESSION_DEPENDENCY);
      return { ok: false, message: "ui.pin.unreachable" } as const;
    }
    if (outcome.kind === "rejected") return { ok: false, message: "ui.pin.rejected" } as const;
    if (outcome.kind === "locked") {
      return { ok: false, message: "ui.pin.locked", retryAfterS: outcome.retryAfterS } as const;
    }
    if (outcome.kind !== "ticket") return { ok: false, message: "ui.pin.unreachable" } as const;
    approving = null;
    void run(job, outcome.ticket);
    return { ok: true } as const;
  }

  function save(event: SubmitEvent) {
    event.preventDefault();
    const price = parsePrice(form.price, exponent);
    const rate = form.tax.trim() === "" ? undefined : parsePct(form.tax);
    const bad: Record<string, MessageKey> = {};
    const components = form.bundle
      ? form.parts.map((c) => ({
          product_id: c.id,
          qty: /^\d{1,4}$/.test(c.qty.trim()) ? Number(c.qty.trim()) : 0,
        }))
      : [];
    if (form.bundle && (!components.length || components.some((c) => !c.product_id || c.qty < 1))) {
      bad.components = INVALID.components;
    }
    if (price === null) bad.price_minor = INVALID.price_minor;
    if (rate === null) bad.tax_rate_bp = INVALID.tax_rate_bp;
    if (Object.keys(bad).length) {
      errors = bad;
      return void tick().then(() =>
        formEl?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus(),
      );
    }
    const payload: IntentPayload = {
      kind: "product.upsert",
      id: editing?.id,
      name: form.name,
      category: form.category,
      price_minor: price!,
      tax_rate_bp: rate ?? undefined,
      hsn: form.hsn,
      track_stock: form.track && !form.bundle,
      image: form.image,
      barcode: form.barcode,
      kitchen: form.kitchen,
      favourite: form.favourite,
      active: form.active,
      bundle: form.bundle,
      components,
    };
    void run({ payload, rev: editing?.rev, key: keyFor(payload) });
  }

  const money = (minor: number) => {
    const parts = [symbol, priceText(minor, exponent)];
    return (branchLocale.current?.symbol_position === "after" ? parts.reverse() : parts).join("");
  };
</script>

<svelte:head>
  <title>{t("hub.page_title", { screen: t("hub.nav.inventory") })}</title>
</svelte:head>

<section class="products" aria-labelledby="products-title">
  <h1 class="type-hub-numeric" id="products-title">{t("hub.nav.inventory")}</h1>
  <h2 class="type-hub-title">{t("hub.products.title")}</h2>
  <p class="intro type-hub-body">{t("hub.products.intro")}</p>
  <a class="link type-hub-label" href="/inventory/stock-in">{t("hub.products.stock_link")}</a>
  <a class="link type-hub-label" href="/inventory/waste">{t("hub.products.waste_link")}</a>
  <a class="link type-hub-label" href="/inventory/expiring">{t("hub.products.expiring_link")}</a>
  <a class="link type-hub-label" href="/inventory/expired">{t("hub.products.expired_link")}</a>

  <form class="form" bind:this={formEl} onsubmit={save} data-testid="product-form">
    <TextField label="hub.products.name" bind:value={form.name} error={errors.name} required />
    <TextField label="hub.products.category" bind:value={form.category} error={errors.category} />
    <TextField
      label="hub.products.price"
      bind:value={form.price}
      error={errors.price_minor}
      inputmode="decimal"
      required
    />
    <TextField
      label="hub.products.tax"
      bind:value={form.tax}
      error={errors.tax_rate_bp}
      inputmode="decimal"
    />
    {#if tax}
      <p class="hint type-hub-label">{t("hub.products.tax_hint", { rate: pct(defaultRate) })}</p>
    {/if}
    <TextField
      label="hub.products.hsn"
      bind:value={form.hsn}
      error={errors.hsn}
      inputmode="numeric"
    />
    <TextField label="hub.products.barcode" bind:value={form.barcode} error={errors.barcode} />
    <div class="field type-hub-label">
      <label for="product-image">{t("hub.products.image")}</label>
      <input
        id="product-image"
        class="ui-interactive file"
        type="file"
        accept="image/*"
        aria-invalid={errors.image ? "true" : undefined}
        aria-describedby={errors.image ? "product-image-error" : undefined}
        onchange={pick}
      />
      {#if errors.image}<p class="error" id="product-image-error" role="alert">
          {t(errors.image)}
        </p>{/if}
      {#if form.image}
        <img class="preview" src={form.image} alt="" data-testid="product-image" />
        <button
          type="button"
          class="ui-interactive btn type-hub-label"
          onclick={() => (form.image = "")}>{t("hub.products.image_remove")}</button
        >
      {/if}
    </div>
    <label class="check type-hub-label"
      ><input type="checkbox" bind:checked={form.bundle} />{t("hub.products.bundle")}</label
    >
    {#if errors.bundle}<p class="error" role="alert">{t(errors.bundle)}</p>{/if}
    {#if form.bundle}
      <fieldset class="field type-hub-label" data-testid="bundle-components">
        <legend>{t("hub.products.components")}</legend>
        {#each form.parts as part, i (i)}
          <div class="part">
            <select
              class="ui-interactive select"
              aria-label={t("hub.products.component", { n: i + 1 })}
              bind:value={part.id}
            >
              <option value="">{t("hub.products.component_pick")}</option>
              {#each options as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
            </select>
            <input
              class="ui-interactive qty"
              inputmode="numeric"
              aria-label={t("hub.products.component_qty", { n: i + 1 })}
              bind:value={part.qty}
            />
            <button
              type="button"
              class="ui-interactive btn type-hub-label"
              onclick={() => (form.parts = form.parts.filter((_, j) => j !== i))}
              >{t("hub.products.component_remove", { n: i + 1 })}</button
            >
          </div>
        {/each}
        {#if errors.components}<p class="error" role="alert">{t(errors.components)}</p>{/if}
        <button
          type="button"
          class="ui-interactive btn type-hub-label"
          onclick={() => (form.parts = [...form.parts, { id: "", qty: "1" }])}
          >{t("hub.products.component_add")}</button
        >
      </fieldset>
    {:else}
      <label class="check type-hub-label"
        ><input type="checkbox" bind:checked={form.track} />{t("hub.products.track")}</label
      >
    {/if}
    <label class="check type-hub-label"
      ><input type="checkbox" bind:checked={form.kitchen} />{t("hub.products.kitchen")}</label
    >
    <label class="check type-hub-label"
      ><input type="checkbox" bind:checked={form.favourite} />{t("hub.products.favourite")}</label
    >
    <label class="check type-hub-label"
      ><input type="checkbox" bind:checked={form.active} />{t("hub.products.active")}</label
    >
    <div class="actions">
      <Button label={editing ? "hub.products.save" : "hub.products.add"} type="submit" />
      {#if editing}<Button
          label="hub.products.cancel"
          variant="secondary"
          onclick={() => reset()}
        />{/if}
    </div>
  </form>

  <p
    class="status type-hub-label"
    class:refusal={status?.tone === "refusal"}
    role="status"
    data-testid="product-status"
  >
    {#if status}
      {#if status.tone === "refusal"}<span aria-hidden="true">⚠</span>{/if}
      <span>{t(status.message)}</span>
    {/if}
  </p>

  {#if approving}
    <div class="approval" data-testid="product-approval">
      <p class="type-hub-label">{t("hub.products.approves")}</p>
      <InlinePin message={approvalPrompt} onsubmit={approve} oncancel={() => (approving = null)} />
    </div>
  {/if}

  <h2 class="type-hub-title">{t("hub.products.list")}</h2>
  {#if loadFailed}
    <p class="type-hub-body" role="alert">{t("hub.products.load_failed")}</p>
    <Button label="common.retry" variant="secondary" onclick={() => void load()} />
  {:else if loading}
    <p class="type-hub-body" aria-busy="true">…</p>
  {:else}
    {#if categories.length}
      <label class="field type-hub-label">
        <span>{t("hub.products.filter")}</span>
        <select class="ui-interactive select" bind:value={filter}>
          <option value="">{t("hub.products.all")}</option>
          {#each categories as c (c)}<option value={c}>{c}</option>{/each}
        </select>
      </label>
    {/if}
    {#if shown.length === 0}
      <p class="type-hub-body">{t("hub.products.empty")}</p>
    {:else}
      <ul class="rows" data-testid="product-list">
        {#each shown as p (p.id)}
          <li class="row" data-testid="product-row" data-product-id={p.id}>
            {#if p.image}<img class="thumb" src={p.image} alt="" />{/if}
            <span class="type-hub-title">{p.name}</span>
            <span class="type-hub-label">{p.category}</span>
            <span class="type-hub-label"
              >{t("hub.products.price_tax", {
                price: money(p.price_minor),
                rate: pct(p.tax_rate_bp),
              })}</span
            >
            {#if p.barcode}<span class="type-hub-label">{p.barcode}</span>{/if}
            {#if p.bundle}<span class="badge type-hub-label">{t("hub.products.badge.bundle")}</span>
              <span class="type-hub-label"
                >{t("hub.products.component_list", {
                  items: p.components
                    .map((c) => t("hub.products.component_item", { qty: c.qty, name: c.name }))
                    .join(", "),
                })}</span
              >
              {#if p.reason === "component_inactive"}<span class="badge type-hub-label"
                  >{t("hub.products.component_inactive", {
                    names: p.components
                      .filter((c) => !c.active)
                      .map((c) => c.name)
                      .join(", "),
                  })}</span
                >{/if}
            {/if}
            {#if p.kitchen}<span class="badge type-hub-label"
                >{t("hub.products.badge.kitchen")}</span
              >{/if}
            {#if p.favourite}<span class="badge type-hub-label"
                >{t("hub.products.badge.favourite")}</span
              >{/if}
            {#if !p.active}<span class="badge type-hub-label"
                >{t("hub.products.badge.inactive")}</span
              >
            {:else if p.reason === "out_of_stock"}<span class="badge type-hub-label"
                >{t("hub.products.badge.out_of_stock")}</span
              >{/if}
            <div class="actions">
              <button
                type="button"
                class="ui-interactive btn type-hub-label"
                onclick={() => reset(p)}>{t("hub.products.edit", { name: p.name })}</button
              >
              <button
                type="button"
                class="ui-interactive btn type-hub-label"
                onclick={() => void run(deletion(p))}
                >{t("hub.products.delete", { name: p.name })}</button
              >
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<style>
  .products,
  .form {
    display: grid;
    gap: var(--space-3);
    max-width: 48rem;
  }

  h1,
  h2,
  p {
    margin: 0;
  }

  .link {
    color: var(--text);
  }

  .intro,
  .hint {
    color: var(--text-muted);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  .error {
    color: var(--text-field-error-text);
  }

  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--space-touch-min);
  }

  .check input {
    inline-size: var(--space-touch-min);
    block-size: var(--space-touch-min);
  }

  .select,
  .file {
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
  }

  .part {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .qty {
    inline-size: 6rem;
    min-height: var(--space-touch-min);
    padding: 0 var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    background-color: var(--surface-2);
    color: var(--text);
  }

  .preview,
  .thumb {
    max-inline-size: 8rem;
    max-block-size: 8rem;
    border-radius: var(--radius-md);
  }

  .thumb {
    max-inline-size: 3rem;
    max-block-size: 3rem;
  }

  .rows {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--line);
    border-radius: var(--radius-md);
    background-color: var(--surface);
  }

  .badge {
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-inline-start: auto;
  }

  .form .actions {
    margin-inline-start: 0;
  }

  .btn {
    min-height: var(--space-touch-min);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-full);
    background-color: var(--surface-2);
    color: var(--text);
    cursor: pointer;
  }

  .status {
    display: flex;
    gap: var(--space-2);
    min-height: 1lh;
  }

  .status.refusal {
    color: var(--text-field-error-text);
  }

  .approval {
    display: grid;
    gap: var(--space-2);
  }
</style>
