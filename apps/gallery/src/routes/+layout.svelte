<script lang="ts">
  import "@arena/ui/base.css";
  import { setPseudo } from "@arena/ui";
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();

  // `?pseudo=1` switches every message to the +30% accented pseudo-locale.
  setPseudo(new URLSearchParams(window.location.search).get("pseudo") === "1");
</script>

{@render children()}
