<script lang="ts">
  import { t } from "@arena/ui";
  import type { MessageKey } from "@arena/ui/messages";
  import type { PrinterState } from "@arena/ui/generated";
  import { skipPrinter } from "./api";
  import HubAge from "./HubAge.svelte";
  import SkipNote from "./SkipNote.svelte";
  import { watchSetup } from "./setupView.svelte";
  import { STEP_NAMES } from "./wizard.svelte";

  const STATES: Record<PrinterState, MessageKey> = {
    ok: "onboarding.printer.state.ok",
    failed: "onboarding.printer.state.failed",
    skipped: "onboarding.printer.state.skipped",
    unset: "onboarding.printer.state.unset",
  };
  const w = watchSetup();
  const printer = $derived(w.view?.setup?.status?.printer);
</script>

<h1 tabindex="-1">{t(STEP_NAMES.printer_payments)}</h1>
<p>{t("onboarding.printer.intro")}</p>
<HubAge view={w.view} />
{#if printer}
  <p data-testid="printer-state">{t(STATES[printer])}</p>
{/if}
<SkipNote skip={skipPrinter} note="onboarding.printer.skip_note" />
