<script lang="ts">
  import { t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import { brandsOf, CONNECTIONS, filterHardware, isTested, type HardwareRow } from "../lib/help";
  import { releaseDate } from "../lib/releases";

  /**
   * The printers and cash drawers from the versioned data file, filterable by brand and
   * connection. Every row gets its category's label, so a new category is a new row, not a new
   * page. A "Tested device" badge marks a device printed from on a Windows and an Android Hub.
   */
  let { rows }: { rows: HardwareRow[] } = $props();

  let brand = $state("");
  let connection = $state("");

  const brands = $derived(brandsOf(rows));
  const shown = $derived(filterHardware(rows, brand, connection));
  const key = (prefix: string, value: string) => `${prefix}.${value}` as MessageKey; // i18n-ignore
</script>

<div class="hw" data-component="hardware-table">
  <div class="hw-filters" role="group" aria-label={t("site.help.hardware.filters")}>
    <div class="hw-filter">
      <label for="hw-brand">{t("site.help.hardware.brand")}</label>
      <select id="hw-brand" bind:value={brand}>
        <option value="">{t("site.help.hardware.all_brands")}</option>
        {#each brands as name (name)}
          <option value={name}>{name}</option>
        {/each}
      </select>
    </div>
    <div class="hw-filter">
      <label for="hw-connection">{t("site.help.hardware.connection")}</label>
      <select id="hw-connection" bind:value={connection}>
        <option value="">{t("site.help.hardware.all_connections")}</option>
        {#each CONNECTIONS as value (value)}
          <option {value}>{t(key("site.help.hardware.connection", value))}</option>
        {/each}
      </select>
    </div>
  </div>

  <p class="visually-hidden" role="status" aria-live="polite">
    {t(shown.length === 1 ? "site.help.hardware.count_one" : "site.help.hardware.count_other", {
      count: shown.length,
    })}
  </p>

  {#if shown.length === 0}
    <p class="hw-empty">{t("site.help.hardware.no_match")}</p>
  {:else}
    <div class="hw-scroll" role="region" aria-label={t("site.help.hardware.caption")} tabindex="0">
      <table>
        <caption class="visually-hidden">{t("site.help.hardware.caption")}</caption>
        <thead>
          <tr>
            <th scope="col">{t("site.help.hardware.col.device")}</th>
            <th scope="col">{t("site.help.hardware.col.type")}</th>
            <th scope="col">{t("site.help.hardware.col.connection")}</th>
            <th scope="col">{t("site.help.hardware.col.paper")}</th>
            <th scope="col">{t("site.help.hardware.col.result")}</th>
            <th scope="col">{t("site.help.hardware.col.checked")}</th>
          </tr>
        </thead>
        <tbody>
          {#each shown as row (row.category + row.brand + row.model + row.connection + row.paper_mm)}
            <tr data-category={row.category}>
              <th scope="row">
                {row.brand}
                {row.model}
                {#if isTested(row)}
                  <span class="hw-badge">{t("site.help.hardware.tested")}</span>
                {/if}
              </th>
              <td>{t(key("site.help.hardware.type", row.category))}</td>
              <td>{t(key("site.help.hardware.connection", row.connection))}</td>
              <td>
                {row.paper_mm === null ? "" : t("site.help.hardware.paper", { mm: row.paper_mm })}
              </td>
              <td>{t(key("site.help.hardware.result", row.result))}</td>
              <td>{releaseDate(row.checked)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
