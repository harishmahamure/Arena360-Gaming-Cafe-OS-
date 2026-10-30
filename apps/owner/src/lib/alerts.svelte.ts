import { t } from "@arena/ui";
import type { OwnerAlertList, OwnerAlertSettings, OwnerAlertView } from "@arena/ui/generated";
import { load, send } from "./api";
import { refresh } from "./shell.svelte";

/** How often a visible Alerts tab reads the list again. */
export const POLL_MS = 30_000;

export const alerts = $state({
  list: [] as OwnerAlertView[],
  /** Pass as `before` to read the next, older page; empty on the last page. */
  more: "",
  loaded: false,
  failed: false,
  /** The announcement of alerts that arrived since the tab was last read (one polite live region). */
  announcement: "",
  settings: null as OwnerAlertSettings | null,
  settingsFailed: false,
});

// The alerts this device has already listed: only an id outside it is announced, once. The first
// read only fills it, so opening the tab does not read the whole history aloud.
const known = new Set<string>();
let primed = false;
let ticket = 0;

function announce(fresh: OwnerAlertView[]) {
  if (!primed) {
    primed = true;
    for (const a of fresh) known.add(a.id);
    return;
  }
  const news = fresh.filter((a) => !known.has(a.id));
  for (const a of fresh) known.add(a.id);
  if (news.length) {
    alerts.announcement = news
      .reverse()
      .map((a) => t("owner.alerts.announce", { branch: a.branch_name, text: a.text }))
      .join(" ");
  }
}

/** Reads the newest page. A signed-out answer re-reads the session (which sends the owner to sign in). */
export async function refreshAlerts() {
  const mine = ++ticket;
  const res = await load<OwnerAlertList>("/alerts");
  if (mine !== ticket) return;
  if (res === "signed-out") {
    await refresh();
    return;
  }
  if (!res) {
    alerts.failed = true;
    return;
  }
  alerts.failed = false;
  alerts.loaded = true;
  if (res.live) announce(res.data.alerts);
  // Older pages already shown stay below the newest page.
  const newest = new Set(res.data.alerts.map((a) => a.id));
  const oldest = res.data.alerts.at(-1);
  const kept = oldest
    ? alerts.list.filter((a) => !newest.has(a.id) && a.created_at <= oldest.created_at)
    : [];
  alerts.list = [...res.data.alerts, ...kept];
  if (!kept.length) alerts.more = res.data.next_before ?? "";
}

/** Reads the next, older page. */
export async function olderAlerts() {
  if (!alerts.more) return;
  const res = await load<OwnerAlertList>(`/alerts?before=${encodeURIComponent(alerts.more)}`);
  if (res === "signed-out") return void (await refresh());
  if (!res) {
    alerts.failed = true;
    return;
  }
  const have = new Set(alerts.list.map((a) => a.id));
  for (const a of res.data.alerts) known.add(a.id);
  alerts.list = [...alerts.list, ...res.data.alerts.filter((a) => !have.has(a.id))];
  alerts.more = res.data.next_before ?? "";
}

export async function loadSettings() {
  const res = await load<OwnerAlertSettings>("/alerts/settings");
  if (res === "signed-out") return void (await refresh());
  alerts.settingsFailed = !res;
  if (res) alerts.settings = res.data;
}

/** Saves the WhatsApp switch; answers whether the cloud took it. */
export async function setWhatsapp(whatsapp: boolean): Promise<boolean> {
  const res = await send<OwnerAlertSettings>("PATCH", "/alerts/settings", { whatsapp });
  if (res.ok) alerts.settings = res.data;
  return res.ok;
}
