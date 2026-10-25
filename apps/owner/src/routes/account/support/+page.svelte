<script lang="ts">
  import { t } from "@arena/ui";
  import { PUBLIC_SUPPORT_EMAIL } from "$app/env/public";
</script>

<svelte:head><title>{t("app.title", { surface: t("owner.account.support") })}</title></svelte:head>

<h1 class="type-web-display">{t("owner.account.support")}</h1>
<p>
  {t("owner.support.email")}: <a href="mailto:{PUBLIC_SUPPORT_EMAIL}">{PUBLIC_SUPPORT_EMAIL}</a>
</p>
<p>{t("owner.support.help")}</p>
<a href="/account">{t("owner.account.back")}</a>
