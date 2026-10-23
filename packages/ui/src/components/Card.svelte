<script lang="ts">
  import type { Snippet } from "svelte";

  /** A `surface` container. `context` picks the owner-app or web card padding. */
  let { context = "owner", children }: { context?: "owner" | "web"; children: Snippet } = $props();
</script>

<div class="card card--{context}" data-component="card">
  {@render children()}
</div>

<style>
  .card {
    --ring-gap: var(--card-background);
    box-sizing: border-box;
    max-width: 100%;
    border: 1px solid var(--line);
    border-radius: var(--radius-xl);
    background-color: var(--card-background);
    color: var(--text);
  }

  .card--owner {
    padding: var(--space-owner-card-padding);
  }

  .card--web {
    padding: var(--space-web-card-padding);
  }

  @media (max-width: 839px) {
    .card--owner {
      padding: var(--space-owner-card-padding-phone);
    }

    .card--web {
      padding: var(--space-web-card-padding-phone);
    }
  }
</style>
