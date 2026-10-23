// Compile-time checks run by `svelte-check`: a disabled Button must carry a disabledReason.
import type { ComponentProps } from "svelte";
import type { Button } from "@arena/ui";

export const ok: ComponentProps<typeof Button> = {
  label: "gallery.button.primary",
  disabled: true,
  disabledReason: "gallery.disabled.no_device",
};

// A runtime boolean is fine as long as a reason is supplied.
declare const busy: boolean;
export const dynamic: ComponentProps<typeof Button> = {
  label: "gallery.button.primary",
  disabled: busy,
  disabledReason: "gallery.disabled.no_device",
};

// @ts-expect-error -- disabled without a reason is a type error
export const missingReason: ComponentProps<typeof Button> = {
  label: "gallery.button.primary",
  disabled: true,
};

// @ts-expect-error -- labels must be catalogue keys
export const unknownLabel: ComponentProps<typeof Button> = { label: "not.a.key" };
