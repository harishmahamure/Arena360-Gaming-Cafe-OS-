<script lang="ts">
  import { TextField, t } from "@arena/ui";
  import { searchHelp, type SearchEntry } from "../lib/help";

  /**
   * Search over the index built at deploy and handed in with the page: it runs in the browser with
   * no server call. Matches are articles whose title or summary hold every word typed.
   */
  let { index }: { index: SearchEntry[] } = $props();

  let term = $state("");
  const results = $derived(searchHelp(index, term));
  const searching = $derived(term.trim() !== "");
</script>

<div class="help-search" data-component="help-search" role="search">
  <TextField label="site.help.search.label" type="search" bind:value={term} autocomplete="off" />

  <div class="help-search-results" role="status" aria-live="polite">
    {#if searching && results.length === 0}
      <p class="help-search-none">{t("site.help.search.none")}</p>
    {:else if searching}
      <p class="visually-hidden">
        {t(results.length === 1 ? "site.help.search.count_one" : "site.help.search.count_other", {
          count: results.length,
        })}
      </p>
    {/if}
  </div>

  {#if searching && results.length > 0}
    <ul class="help-list" aria-label={t("site.help.search.results")}>
      {#each results as entry (entry.slug)}
        <li>
          <a href={`/help/${entry.slug}`}>{entry.title}</a>
          <p>{entry.summary}</p>
        </li>
      {/each}
    </ul>
  {/if}
</div>
