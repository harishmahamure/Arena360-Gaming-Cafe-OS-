<script lang="ts">
  import { Button, t } from "@arena/ui";
  import { onMount } from "svelte";
  import { LEGAL_PARAGRAPHS, type LegalPage } from "./signup";

  /**
   * A read-only legal page (Story 6.10): the Terms or the Privacy Policy as short bundled text, so
   * the kiosk never needs the internet. "Back" returns to the sign-up form with its fields kept.
   */
  let {
    page,
    onback,
  }: {
    page: LegalPage;
    onback: () => void;
  } = $props();

  const title = $derived(t(page === "terms" ? "player.signup.terms" : "player.signup.privacy"));

  let root: HTMLElement | undefined = $state();

  onMount(() => root?.focus());
</script>

<section
  class="legal"
  data-testid="legal-page"
  data-page={page}
  aria-labelledby="legal-title"
  tabindex="-1"
  bind:this={root}
>
  <h2 class="type-kiosk-title" id="legal-title">{title}</h2>
  <div class="body">
    {#each LEGAL_PARAGRAPHS[page] as key (key)}
      <p class="type-kiosk-body">{t(key)}</p>
    {/each}
  </div>
  <div class="actions">
    <Button variant="secondary" label="player.legal.back" onclick={onback} />
  </div>
</section>

<style>
  .legal {
    display: grid;
    gap: var(--space-4);
    justify-items: stretch;
    width: min(32rem, 80vw);
    text-align: start;
  }

  .legal:focus {
    outline: none;
  }

  .legal > * {
    margin: 0;
  }

  h2 {
    text-align: center;
  }

  .body {
    display: grid;
    gap: var(--space-3);
  }

  .body > p {
    margin: 0;
  }

  .actions {
    display: flex;
    justify-content: flex-start;
  }
</style>
