<script lang="ts">
  import { Button, ThemeToggle, t } from "@arena/ui";
  import { PUBLIC_ONBOARDING_URL } from "$app/env/public";
  import { signInUrl, signOut } from "../../lib/api";

  const LINKS = [
    { href: "/account/subscription", label: "owner.account.subscription" },
    { href: "/account/branches", label: "owner.account.branches" },
    { href: "/account/support", label: "owner.account.support" },
  ] as const;

  async function out() {
    await signOut();
    location.assign(signInUrl());
  }
</script>

<svelte:head><title>{t("app.title", { surface: t("owner.nav.account") })}</title></svelte:head>

<h1 class="type-web-display">{t("owner.nav.account")}</h1>
<ul class="links">
  <li>
    <a class="link" href="{PUBLIC_ONBOARDING_URL.replace(/\/$/, '')}/account/security"
      >{t("owner.account.mfa")}</a
    >
  </li>
  {#each LINKS as link (link.href)}
    <li><a class="link" href={link.href}>{t(link.label)}</a></li>
  {/each}
</ul>
<ThemeToggle />
<p><Button label="owner.account.sign_out" variant="secondary" onclick={out} /></p>

<style>
  .links {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: 0;
    list-style: none;
  }

  .link {
    display: flex;
    align-items: center;
    min-height: var(--space-touch-min);
  }
</style>
