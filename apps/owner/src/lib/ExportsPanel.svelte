<script lang="ts">
  import { Button, Card, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { EXPORTS, type ExportKind } from "./reports";

  let {
    from,
    to,
    branches,
    busy,
    csv,
    pdf,
  }: {
    from: string;
    to: string;
    branches: string;
    busy: boolean;
    csv: (kind: ExportKind) => void;
    pdf: (kind: ExportKind) => void;
  } = $props();

  const NAMES: Record<ExportKind, { title: MessageKey; note: MessageKey }> = {
    revenue: { title: "owner.reports.tab.revenue", note: "owner.reports.exports.revenue" },
    cash: { title: "owner.reports.tab.cash", note: "owner.reports.exports.cash" },
    stock: { title: "owner.reports.tab.stock", note: "owner.reports.exports.stock" },
    credit: { title: "owner.reports.tab.credit", note: "owner.reports.exports.credit" },
  };
</script>

<p class="scope">{t("owner.reports.exports.scope", { from, to, branches })}</p>
<ul class="list">
  {#each EXPORTS as kind (kind)}
    <li>
      <Card>
        <h2 class="type-hub-label">{t(NAMES[kind].title)}</h2>
        <p class="note">{t(NAMES[kind].note)}</p>
        <div class="actions">
          <Button
            label="owner.reports.export.csv"
            variant="secondary"
            onclick={() => csv(kind)}
            disabled={busy}
            disabledReason="owner.reports.export.busy"
          />
          <Button label="owner.reports.export.pdf" variant="secondary" onclick={() => pdf(kind)} />
        </div>
      </Card>
    </li>
  {/each}
</ul>

<style>
  .scope {
    margin: 0;
    color: var(--text-muted);
  }
  .list {
    display: grid;
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  h2 {
    margin: 0 0 var(--space-1);
  }
  .note {
    margin: 0 0 var(--space-3);
    color: var(--text-muted);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }
</style>
