<script lang="ts">
  import { Button, Card, t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { OwnerBranch, WizardStep } from "@arena/ui/generated";
  import { setupUrl } from "./api";

  let { branch }: { branch: OwnerBranch } = $props();

  const STEPS: Record<WizardStep, MessageKey> = {
    cafe: "ui.wizard.step.cafe",
    tier: "ui.wizard.step.tier",
    install_hub: "ui.wizard.step.install_hub",
    devices: "ui.wizard.step.devices",
    rates: "ui.wizard.step.rates",
    products: "ui.wizard.step.products",
    staff: "ui.wizard.step.staff",
    printer_payments: "ui.wizard.step.printer_payments",
    test_session: "ui.wizard.step.test_session",
    go_live: "ui.wizard.step.go_live",
  };
</script>

<Card>
  <section class="setup" aria-labelledby="setup-{branch.id}">
    <h2 id="setup-{branch.id}" class="type-hub-title">
      {t("owner.setup.title", {
        branch: branch.name,
        done: branch.setup.steps_done,
        total: branch.setup.steps_total,
      })}
    </h2>
    <p>{t("owner.setup.next", { step: t(STEPS[branch.setup.next_step]) })}</p>
    <Button label="owner.branches.continue" onclick={() => location.assign(setupUrl(branch.id))} />
  </section>
</Card>

<style>
  .setup {
    display: grid;
    gap: var(--space-3);
    justify-items: start;
  }
  h2,
  p {
    margin: 0;
  }
</style>
