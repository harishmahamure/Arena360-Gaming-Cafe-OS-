<script lang="ts">
  import { Placeholder, t } from "@arena/ui";
</script>

<svelte:head>
  <title>{t("app.title", { surface: t("surface.onboarding") })}</title>
</svelte:head>

<Placeholder surface={t("surface.onboarding")} />
