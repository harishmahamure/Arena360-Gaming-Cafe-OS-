<script lang="ts">
  import { t, type MessageKey } from "../messages/index.js";
  import type { WizardStep, WizardStepInfo } from "../generated";

  let {
    steps,
    current,
    href,
  }: {
    steps: WizardStepInfo[];
    current: WizardStep;
    href: (key: WizardStep) => string;
  } = $props();

  const NAMES: Record<WizardStep, MessageKey> = {
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

  const total = $derived(steps.length);
  const index = $derived(
    Math.max(
      0,
      steps.findIndex((s) => s.key === current),
    ),
  );
  const currentName = $derived(t(NAMES[current]));
</script>

<nav class="progress" aria-label={t("ui.wizard.label")} data-component="wizard-progress">
  <div class="wide">
    <p class="count">{t("ui.wizard.step_of", { n: index + 1, total })}</p>
    <ol class="rows">
      {#each steps as step, i (step.key)}
        {@const isCurrent = step.key === current}
        {@const done = step.state === "done"}
        <li class="row" class:current={isCurrent} data-step={step.key} data-state={step.state}>
          {#if done && !isCurrent}
            <a class="ui-interactive cell link" href={href(step.key)}>
              <span class="mark" aria-hidden="true">✓</span>
              <span class="name">{t(NAMES[step.key])}</span>
              <span class="visually-hidden">{t("ui.wizard.done")}</span>
              {#if step.can_skip}<span class="tag">{t("ui.wizard.can_skip")}</span>{/if}
            </a>
          {:else if isCurrent}
            <a class="ui-interactive cell link" href={href(step.key)} aria-current="step">
              <span class="mark" aria-hidden="true">{done ? "✓" : i + 1}</span>
              <span class="name">{t(NAMES[step.key])}</span>
              {#if done}<span class="visually-hidden">{t("ui.wizard.done")}</span>{/if}
              {#if step.can_skip}<span class="tag">{t("ui.wizard.can_skip")}</span>{/if}
            </a>
          {:else}
            <span class="cell">
              <span class="mark" aria-hidden="true">{done ? "✓" : i + 1}</span>
              <span class="name">{t(NAMES[step.key])}</span>
              {#if done}<span class="visually-hidden">{t("ui.wizard.done")}</span>{/if}
              {#if step.can_skip}<span class="tag">{t("ui.wizard.can_skip")}</span>{/if}
            </span>
          {/if}
        </li>
      {/each}
    </ol>
  </div>
  <div class="narrow">
    <ol class="dots" aria-hidden="true">
      {#each steps as step (step.key)}
        <li
          class="dot"
          class:dot-done={step.state === "done"}
          class:dot-current={step.key === current}
        ></li>
      {/each}
    </ol>
    <p class="count">
      {t("ui.wizard.step_of_named", { n: index + 1, total, name: currentName })}
    </p>
  </div>
</nav>

<style>
  .progress {
    min-width: 0;
  }

  .count {
    margin: 0 0 var(--space-2);
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
    font-weight: var(--type-hub-title-weight);
    line-height: var(--type-hub-label-line-height);
  }

  .rows,
  .dots {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .cell {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: var(--space-touch-min);
    box-sizing: border-box;
    padding: 0 var(--space-3);
    border-radius: var(--radius-sm);
    color: var(--text-muted);
  }

  .link {
    color: var(--text);
    text-decoration: none;
  }

  .current .cell {
    background: var(--surface-2);
    color: var(--text);
    font-weight: var(--type-hub-title-weight);
  }

  .mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 1.5rem;
    height: 1.5rem;
    border: 1px solid var(--line);
    border-radius: 50%;
    font-size: var(--type-hub-label-size);
  }

  [data-state="done"] .mark {
    border-color: var(--ok);
    color: var(--ok);
  }

  .name {
    flex: 1;
    min-width: 0;
  }

  .tag {
    flex: none;
    padding: 0 var(--space-2);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    font-size: var(--type-hub-label-size);
    font-weight: normal;
  }

  .narrow {
    display: none;
  }

  .dots {
    display: flex;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  .dot {
    width: 0.625rem;
    height: 0.625rem;
    border: 1px solid var(--text-muted);
    border-radius: 50%;
    box-sizing: border-box;
  }

  .dot-done {
    border-color: var(--ok);
    background: var(--ok);
  }

  .dot-current {
    border-color: var(--accent);
    background: var(--accent);
  }

  @media (max-width: 839.98px) {
    .wide {
      display: none;
    }

    .narrow {
      display: block;
    }
  }
</style>
