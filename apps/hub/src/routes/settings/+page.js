import { redirect } from "@sveltejs/kit";

/** `/settings` opens its first section, Branch. */
export function load() {
  redirect(307, "/settings/branch");
}
