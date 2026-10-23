<script lang="ts">
  import { Placeholder, t } from "@arena/ui";
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("surface.hub") })}</title>
</svelte:head>

<Placeholder surface={t("surface.hub")} />
