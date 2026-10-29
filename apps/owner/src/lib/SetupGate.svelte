<script lang="ts">
  import { t } from "@arena/ui";
  import type { OwnerBranch } from "@arena/ui/generated";
  import { setupUrl } from "./api";

  /** What a branch-scoped screen shows for a branch that is not live yet. */
  let { branch }: { branch: OwnerBranch } = $props();
</script>

<p>{t("owner.setup.after_live", { branch: branch.name })}</p>
<a class="link" href={setupUrl(branch.id)}>{t("owner.branches.continue")}</a>

<style>
  p {
    margin: 0 0 var(--space-2);
  }
  .link {
    color: var(--accent-text);
    text-decoration: underline;
  }
</style>
