import type { SetupDevicesView, WizardState } from "@arena/ui/generated";
import { onMount } from "svelte";
import { getDevices, getWizard } from "./api";
import { advance, wizard } from "./wizard.svelte";

const POLL_MS = 5000;

export function watchSetup(hold: (next: WizardState) => number = () => 0) {
  const w = $state<{ view: SetupDevicesView | null }>({ view: null });
  let busy = false;
  async function refresh() {
    if (busy) return;
    busy = true;
    try {
      const res = await getDevices();
      if (res.ok) w.view = res.data;
      const next = await getWizard();
      if (next.ok && next.data.current !== wizard.state?.current) {
        const wait = hold(next.data);
        if (wait) await new Promise((done) => setTimeout(done, wait));
        await advance(next.data);
      }
    } finally {
      busy = false;
    }
  }
  onMount(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(timer);
  });
  return {
    get view() {
      return w.view;
    },
    refresh,
  };
}
