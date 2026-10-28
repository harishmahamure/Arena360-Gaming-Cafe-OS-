<script lang="ts">
  import { Button, t } from "@arena/ui";
  import type { StarterPreview, StarterResult, StarterTemplateInfo } from "@arena/ui/generated";
  import type { MessageKey } from "@arena/ui/messages";
  import { invalidate } from "$app/navigation";
  import { onMount } from "svelte";
  import { SESSION_DEPENDENCY, newIdempotencyKey } from "./auth";
  import {
    commitStarter,
    loadStarterTemplates,
    previewStarter,
    type ImportOutcome,
  } from "./import";
  import { branchLocale } from "./locale.svelte";
  import { priceText } from "./products";

  let { allowed }: { allowed: boolean } = $props();

  const KIND: Record<string, MessageKey> = {
    pack: "hub.plans.kind.pack",
    block: "hub.plans.kind.block",
    pass: "hub.plans.kind.pass",
  };
  const SKIP: Record<string, MessageKey> = {
    exists: "hub.import.starter_skip_exists",
    no_group: "hub.import.starter_skip_no_group",
    invalid: "hub.import.starter_skip_invalid",
  };

  const exponent = $derived(branchLocale.current?.exponent ?? 2);
  const money = (minor: number) => {
    const parts = [branchLocale.current?.symbol ?? "", priceText(minor, exponent)];
    return (branchLocale.current?.symbol_position === "after" ? parts.reverse() : parts).join("");
  };

  let templates: StarterTemplateInfo[] = $state([]);
  let loading = $state(true);
  let selected: string | null = $state(null);
  let importId = "";
  let checking = $state(false);
  let adding = $state(false);
  let preview: StarterPreview | null = $state(null);
  let result: StarterResult | null = $state(null);
  let problem: string | null = $state(null);

  const busy = $derived(checking || adding);
  const chosen = $derived(templates.find((x) => x.id === selected) ?? null);

  /** Sorts out the shared outcomes; `true` when `outcome` holds a view. */
  async function settled<T>(outcome: ImportOutcome<T>): Promise<T | null> {
    if (outcome.kind === "ok") return outcome.view;
    if (outcome.kind === "signed_out") await invalidate(SESSION_DEPENDENCY);
    else if (outcome.kind === "invalid")
      problem = t("hub.import.refused", { message: outcome.message });
    else problem = t(outcome.kind === "denied" ? "hub.import.no_access" : "hub.import.unreachable");
    return null;
  }

  onMount(async () => {
    if (!allowed) {
      loading = false;
      return;
    }
    const list = await settled(await loadStarterTemplates());
    loading = false;
    if (list) templates = list;
    else if (!problem) problem = t("hub.import.starter_unreachable");
  });

  async function choose(id: string) {
    selected = id;
    result = preview = null;
    problem = null;
    importId = newIdempotencyKey();
    checking = true;
    const view = await settled(await previewStarter(id));
    checking = false;
    if (selected === id && view) preview = view;
  }

  async function add() {
    if (!selected || !preview || busy) return;
    problem = null;
    adding = true;
    const outcome = await commitStarter(selected, importId);
    adding = false;
    const view = await settled(outcome);
    if (!view) {
      if (outcome.kind === "unreachable") problem = t("hub.import.failed");
      return;
    }
    result = view;
    importId = newIdempotencyKey();
    checking = true;
    const again = await settled(await previewStarter(selected));
    checking = false;
    preview = again;
  }

  const rows = (p: StarterPreview) => [
    ...p.plans.map((i) => ({ ...i, what: "hub.import.starter_kind_plan" as MessageKey })),
    ...p.products.map((i) => ({ ...i, what: "hub.import.starter_kind_product" as MessageKey })),
  ];
</script>

<div class="starter">
  <h3 class="type-hub-body heading">{t("hub.import.starter_title")}</h3>
  {#if !allowed}
    <p class="type-hub-body muted" data-testid="import-no-access">
      {t("hub.import.starter_no_access")}
    </p>
  {:else}
    <p class="type-hub-body muted">{t("hub.import.starter_intro")}</p>

    {#if loading}
      <p class="type-hub-label muted" role="status">{t("hub.import.starter_loading")}</p>
    {:else if templates.length > 0}
      <fieldset class="choices" disabled={busy}>
        <legend class="type-hub-label">{t("hub.import.starter_choose")}</legend>
        {#each templates as tpl (tpl.id)}
          <!-- i18n-ignore -->
          <label class="choice" data-testid={`starter-${tpl.id}`}>
            <input
              type="radio"
              name="starter-template"
              value={tpl.id}
              checked={selected === tpl.id}
              onchange={() => void choose(tpl.id)}
            />
            <span>
              <span class="type-hub-body name">{tpl.name}</span>
              <span class="type-hub-label muted">{tpl.description}</span>
            </span>
          </label>
        {/each}
      </fieldset>
    {/if}

    {#if chosen}
      <div class="lists">
        <details open>
          <summary class="type-hub-label">
            {t("hub.import.starter_plans", { count: chosen.plans.length })}
          </summary>
          <ul class="type-hub-body" data-testid="starter-plans">
            {#each chosen.plans as plan (plan.name)}
              <li>
                {t("hub.import.starter_plan_line", {
                  name: plan.name,
                  kind: t(KIND[plan.plan_kind] ?? "hub.import.starter_kind_plan"),
                  price: money(plan.price_minor),
                })}
              </li>
            {/each}
          </ul>
        </details>
        <details open>
          <summary class="type-hub-label">
            {t("hub.import.starter_products", { count: chosen.products.length })}
          </summary>
          <ul class="type-hub-body" data-testid="starter-products">
            {#each chosen.products as product (product.name)}
              <li>
                {t("hub.import.starter_product_line", {
                  name: product.name,
                  category: product.category,
                  price: money(product.price_minor),
                })}
              </li>
            {/each}
          </ul>
        </details>
      </div>
    {/if}

    <div role="status" class="outcome">
      {#if checking}
        <p class="type-hub-label muted">{t("hub.import.checking")}</p>
      {:else if adding}
        <p class="type-hub-label muted">{t("hub.import.importing")}</p>
      {/if}
      {#if problem}
        <p class="type-hub-label warn" data-testid="import-problem">
          <span aria-hidden="true">⚠</span>
          {problem}
        </p>
      {/if}
      {#if result}
        <p class="type-hub-body" data-testid="import-done">
          {t("hub.import.starter_done", {
            plans: result.plans_created,
            products: result.products_created,
            skipped: result.skipped_plans + result.skipped_products,
          })}
        </p>
      {/if}
    </div>

    {#if preview}
      <div class="preview" data-testid="import-preview">
        <p class="type-hub-body" data-testid="import-summary">
          {t("hub.import.starter_summary", {
            newPlans: preview.new_plans,
            newProducts: preview.new_products,
            skippedPlans: preview.skipped_plans,
            skippedProducts: preview.skipped_products,
          })}
        </p>
        {#if !preview.can_commit}
          <p class="type-hub-label muted" data-testid="starter-nothing">
            {t("hub.import.starter_nothing")}
          </p>
        {/if}
        <table data-testid="starter-items">
          <thead>
            <tr class="type-hub-label">
              <th scope="col">{t("hub.import.starter_col_item")}</th>
              <th scope="col">{t("hub.import.starter_col_kind")}</th>
              <th scope="col">{t("hub.import.starter_col_result")}</th>
            </tr>
          </thead>
          <tbody class="type-hub-body">
            {#each rows(preview) as row (`${row.what}:${row.name}`)}
              <tr data-testid="starter-item" data-add={row.add}>
                <td>{row.name}</td>
                <td>{t(row.what)}</td>
                <td>
                  {t(
                    row.add
                      ? "hub.import.starter_will_add"
                      : (SKIP[row.reason ?? ""] ?? "hub.import.starter_skip_invalid"),
                  )}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        <div class="row">
          <Button
            label="hub.import.starter_add"
            disabled={busy || !preview.can_commit}
            disabledReason={busy ? "hub.import.importing" : "hub.import.commit_nothing"}
            onclick={() => void add()}
          />
        </div>
      </div>
    {/if}
  {/if}
</div>

<style>
  .starter {
    display: grid;
    gap: var(--space-3);
  }

  h3,
  p,
  ul {
    margin: 0;
  }

  .muted {
    color: var(--text-muted);
  }

  .warn {
    color: var(--text-field-error-text);
  }

  .choices {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    border: 0;
  }

  .choice {
    display: flex;
    align-items: start;
    gap: var(--space-3);
    min-height: var(--space-touch-min);
  }

  .choice span {
    display: grid;
  }

  .name {
    font-weight: 600;
  }

  .lists {
    display: grid;
    gap: var(--space-2);
  }

  ul {
    padding-inline-start: var(--space-5);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .outcome,
  .preview {
    display: grid;
    gap: var(--space-2);
  }

  table {
    border-collapse: collapse;
    inline-size: 100%;
  }

  th,
  td {
    padding: var(--space-1) var(--space-2);
    border-block-end: 1px solid var(--border-strong);
    text-align: start;
    vertical-align: top;
  }
</style>
