// The "Get started" form rules, the same as the cloud's (`POST /v1/signup/start`): the cloud
// still checks every field and answers `VALIDATION_FAILED` with `field`.

import type { MessageKey } from "@arena/ui/messages";

export type FieldName = "name" | "phone" | "email" | "cafe_name" | "city";
export const FIELDS: readonly FieldName[] = ["name", "phone", "email", "cafe_name", "city"];

export type SignupForm = {
  name: string;
  country: string;
  number: string;
  email: string;
  cafe_name: string;
  city: string;
};

/** Calling codes offered by the country select, with their catalogue names. */
export const COUNTRIES: readonly { code: string; label: MessageKey }[] = [
  { code: "+91", label: "onboarding.country.in" },
  { code: "+971", label: "onboarding.country.ae" },
  { code: "+44", label: "onboarding.country.gb" },
  { code: "+1", label: "onboarding.country.us" },
  { code: "+65", label: "onboarding.country.sg" },
];

const MAX_TEXT = 80;
const MAX_EMAIL = 254;
const MAX_LOCAL = 64;
const LABEL = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/;
const TLD = /^[A-Za-z]{2,}$/;
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** Name, café name or city: 1–80 characters after trim, no control characters. */
export function validText(v: string): boolean {
  const t = v.trim();
  const n = [...t].length;
  return n >= 1 && n <= MAX_TEXT && !CONTROL.test(t);
}

/** E.164: `+`, a non-zero digit, 8–15 digits in all; `+91` needs 10 digits starting 6–9. */
export function validPhone(e164: string): boolean {
  if (!/^\+[1-9]\d{7,14}$/.test(e164)) return false;
  return e164.startsWith("+91") ? /^\+91[6-9]\d{9}$/.test(e164) : true;
}

/** `local@domain.tld` (the cloud's rule; see tests/fixtures/signup-validation.json). */
export function validEmail(raw: string): boolean {
  const e = raw.trim();
  if (e.length === 0 || [...e].length > MAX_EMAIL) return false;
  const parts = e.split("@");
  if (parts.length !== 2) return false;
  const [local, domain] = parts;
  if (local.length === 0 || [...local].length > MAX_LOCAL) return false;
  if (/\s/.test(local) || CONTROL.test(local)) return false;
  const labels = domain.split(".");
  if (labels.length < 2 || !labels.every((l) => LABEL.test(l))) return false;
  return TLD.test(labels[labels.length - 1]);
}

/** The number as typed without spaces, dashes, dots or brackets. */
export const digitsOf = (number: string) => number.replace(/[\s\-.()]/g, "");

/** The E.164 phone the form sends: the calling code plus the cleaned number. */
export const phoneOf = (form: Pick<SignupForm, "country" | "number">) =>
  form.country + digitsOf(form.number).replace(/^0+/, "");

/** The error for one field, or null when it is valid. */
export function fieldError(form: SignupForm, field: FieldName): MessageKey | null {
  switch (field) {
    case "name":
      return validText(form.name) ? null : "onboarding.error.name";
    case "cafe_name":
      return validText(form.cafe_name) ? null : "onboarding.error.cafe_name";
    case "city":
      return validText(form.city) ? null : "onboarding.error.city";
    case "email": {
      return validEmail(form.email) ? null : "onboarding.error.email";
    }
    case "phone": {
      const phone = phoneOf(form);
      if (validPhone(phone)) return null;
      return form.country === "+91" ? "onboarding.error.phone_in" : "onboarding.error.phone";
    }
  }
}

export const isField = (v: unknown): v is FieldName =>
  typeof v === "string" && (FIELDS as readonly string[]).includes(v);

/** `m:ss` for a countdown. */
export function minSec(seconds: number): string {
  const s = Math.max(0, Math.ceil(seconds));
  return String(Math.floor(s / 60)) + ":" + String(s % 60).padStart(2, "0");
}

/** The message for a field the cloud refused (`VALIDATION_FAILED.field`). */
export function serverFieldError(form: SignupForm, field: FieldName): MessageKey {
  const keys: Record<FieldName, MessageKey> = {
    name: "onboarding.error.name",
    phone: form.country === "+91" ? "onboarding.error.phone_in" : "onboarding.error.phone",
    email: "onboarding.error.email",
    cafe_name: "onboarding.error.cafe_name",
    city: "onboarding.error.city",
  };
  return keys[field];
}
