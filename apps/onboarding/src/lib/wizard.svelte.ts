import { t } from "@arena/ui";
import type { MessageKey } from "@arena/ui/messages";
import type { WizardState, WizardStep } from "@arena/ui/generated";
import { goto } from "$app/navigation";
import { tick } from "svelte";

export const STEP_NAMES: Record<WizardStep, MessageKey> = {
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

export const STEP_KEYS = Object.keys(STEP_NAMES) as WizardStep[];

export function isStep(v: string | undefined): v is WizardStep {
  return v !== undefined && (STEP_KEYS as string[]).includes(v);
}

export function stepIndex(key: WizardStep): number {
  return STEP_KEYS.indexOf(key);
}

export const target = $state({ id: "" });

export const stepPath = (key: WizardStep) =>
  `/setup/${key}${target.id ? `?branch=${encodeURIComponent(target.id)}` : ""}`;

class Wizard {
  state = $state<WizardState | null>(null);
  announcement = $state("");
  /** A payment was made and the plan step is waiting for the entitlement (Story 10.4). */
  confirming = $state(false);
}

export const wizard = new Wizard();

export const setupCode = $state<{ asked: boolean; value: string | null }>({
  asked: false,
  value: null,
});

export function clearWizard() {
  setupCode.asked = false;
  setupCode.value = null;
  wizard.state = null;
  wizard.announcement = "";
  wizard.confirming = false;
}

export function setWizard(next: WizardState): boolean {
  const before = wizard.state?.current;
  wizard.state = next;
  const moved = before !== undefined && before !== next.current;
  if (moved) {
    wizard.announcement = t("onboarding.setup.announce", {
      n: stepIndex(next.current) + 1,
      total: next.steps.length,
      name: t(STEP_NAMES[next.current]),
    });
  }
  return moved;
}

export async function focusStepHeading() {
  await tick();
  document.querySelector("main")?.querySelector("h1")?.focus();
}

export async function advance(next: WizardState) {
  setWizard(next);
  await goto(stepPath(next.current));
  await focusStepHeading();
}
