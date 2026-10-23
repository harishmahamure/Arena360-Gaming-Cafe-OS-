<script lang="ts">
  let { surface }: { surface: string } = $props();
</script>

<main class="placeholder">
  <h1>Arena 360</h1>
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
