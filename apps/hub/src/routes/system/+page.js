import { redirect } from "@sveltejs/kit";

/** `/system` opens its first section, Audit. */
export function load() {
  redirect(307, "/system/audit");
}
