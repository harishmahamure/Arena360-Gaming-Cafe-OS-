import { redirect } from "@sveltejs/kit";

/** `/` opens the floor. */
export function load() {
  redirect(307, "/floor");
}
