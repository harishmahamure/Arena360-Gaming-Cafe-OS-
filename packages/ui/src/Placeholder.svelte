<script lang="ts">
  import { t } from "./messages/index.js";

  let { surface }: { surface: string } = $props();
</script>

<main class="placeholder">
  <h1>{t("app.name")}</h1>
  <p>{surface}</p>
</main>

<style>
  .placeholder {
    display: grid;
    place-content: center;
    min-height: 100vh;
    font-family: system-ui, sans-serif;
    text-align: center;
  }
</style>
