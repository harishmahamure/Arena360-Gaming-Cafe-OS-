<script lang="ts">
  import { Placeholder, t } from "@arena/ui";
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("surface.kiosk") })}</title>
</svelte:head>

<Placeholder surface={t("surface.kiosk")} />
