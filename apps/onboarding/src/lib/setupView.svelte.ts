import type { SetupDevicesView } from "@arena/ui/generated";
import { onMount } from "svelte";
import { getDevices, getWizard } from "./api";
import { advance, wizard } from "./wizard.svelte";

const POLL_MS = 5000;

export function watchSetup() {
  const w = $state<{ view: SetupDevicesView | null }>({ view: null });
  async function refresh() {
    const res = await getDevices();
    if (res.ok) w.view = res.data;
    const next = await getWizard();
    if (next.ok && next.data.current !== wizard.state?.current) await advance(next.data);
  }
  onMount(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(timer);
  });
  return w;
}
