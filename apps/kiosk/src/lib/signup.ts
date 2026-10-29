// The kiosk sign-up's rules (Story 6.10): how the service's answer reads, the field checks (the
// same rules the Hub applies at the counter, Story 3.6), the consent text split around its two
// links, and the legal pages. Pure, so the form and its tests share them.
//
// The Hub decides everything: it validates, hashes, creates the player as this device, records the
// login and answers `ok` or a typed refusal. The kiosk never names a player id or a policy version.

import type { ErrorCode, PlayerContext } from "@arena/ui/generated";
import type { MessageKey } from "@arena/ui/messages";

/** The phone field starts with this. */
export const DEFAULT_PHONE = "+91";
/** How long the app waits for the service's answer before it calls the counter unreachable. */
export const SIGNUP_TIMEOUT_MS = 10_000;

/** The fields the form checks, in the order focus goes to the first invalid one. */
export const SIGNUP_FIELDS = ["username", "password", "phone", "name"] as const;
export type SignupField = (typeof SIGNUP_FIELDS)[number];

/** What a sign-up came to. */
export type SignupOutcome =
  | { kind: "ok"; player: PlayerContext }
  | { kind: "refused"; code: ErrorCode; field: string | null }
  | { kind: "unreachable" };

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null;

const isPlayer = (v: unknown): v is PlayerContext =>
  isObject(v) && typeof v.player_id === "string" && typeof v.name === "string";

/**
 * The outcome in the service's answer to `player_signup`: `{ok, code?, body}` where an ok body is
 * the Hub's `player.signup_result`. An error, a missing answer or a body that is neither outcome
 * is `unreachable`.
 */
export function signupOutcomeOf(answer: unknown): SignupOutcome {
  const unreachable: SignupOutcome = { kind: "unreachable" };
  if (!isObject(answer) || answer.ok !== true || !isObject(answer.body)) return unreachable;
  const body = answer.body;
  if (body.outcome === "ok") {
    return isPlayer(body.player) ? { kind: "ok", player: body.player } : unreachable;
  }
  if (body.outcome === "refused" && typeof body.code === "string") {
    return {
      kind: "refused",
      code: body.code as ErrorCode,
      field: typeof body.field === "string" ? body.field : null,
    };
  }
  return unreachable;
}

// The Story 3.6 rules, as the Hub's `PlayerForm::check` applies them.
const USERNAME_MIN = 3;
const USERNAME_MAX = 32;
const PASSWORD_MIN = 6;
const PASSWORD_MAX = 128;
const NAME_MAX = 80;
const PHONE_MIN_DIGITS = 8;
const PHONE_MAX_DIGITS = 15;

/** The username as the Hub reads it: trimmed and lower-cased. */
export const normalizeUsername = (s: string): string => s.trim().toLowerCase();

/** The phone as the Hub reads it: spaces and dashes removed. */
export const normalizePhone = (s: string): string => s.replace(/[ -]/g, "");

// A control character is never allowed in a name.
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;

/** The catalogue key of the message for each field that fails its check. */
export const INVALID: Record<SignupField, MessageKey> = {
  username: "hub.players.invalid.username",
  password: "hub.players.invalid.password",
  phone: "hub.players.invalid.phone",
  name: "hub.players.invalid.name",
};

/** The catalogue key of the message for an invalid field, or `null` when it is fine. */
export function fieldProblem(field: SignupField, value: string): MessageKey | null {
  switch (field) {
    case "username": {
      const u = normalizeUsername(value);
      return u.length >= USERNAME_MIN && u.length <= USERNAME_MAX && /^[a-z0-9._-]+$/.test(u)
        ? null
        : INVALID.username;
    }
    case "password": {
      const n = [...value].length;
      return n >= PASSWORD_MIN && n <= PASSWORD_MAX ? null : INVALID.password;
    }
    case "phone": {
      const p = normalizePhone(value);
      return p.startsWith("+") &&
        /^\d+$/.test(p.slice(1)) &&
        p.length - 1 >= PHONE_MIN_DIGITS &&
        p.length - 1 <= PHONE_MAX_DIGITS
        ? null
        : INVALID.phone;
    }
    case "name": {
      const n = value.trim();
      return n !== "" && [...n].length <= NAME_MAX && !CONTROL.test(n) ? null : INVALID.name;
    }
  }
}

/** Every field's problem (`null` for a fine one). */
export function formProblems(
  values: Record<SignupField, string>,
): Record<SignupField, MessageKey | null> {
  return {
    username: fieldProblem("username", values.username),
    password: fieldProblem("password", values.password),
    phone: fieldProblem("phone", values.phone),
    name: fieldProblem("name", values.name),
  };
}

/** The first field with a problem, in form order, or `null`. */
export function firstInvalid(problems: Record<SignupField, MessageKey | null>): SignupField | null {
  return SIGNUP_FIELDS.find((f) => problems[f] !== null) ?? null;
}

/** Whether `field` from a Hub refusal is one of the form's own fields. */
export const isSignupField = (field: string | null): field is SignupField =>
  field !== null && (SIGNUP_FIELDS as readonly string[]).includes(field);

/** The legal pages the consent text links to (bundled text, read-only). */
export type LegalPage = "terms" | "privacy";

/** One piece of the consent text: plain, or a link to a legal page. */
export type ConsentPart = { text: string; page: LegalPage | null };

/**
 * The consent text split around the first "Terms" and the first "Privacy Policy" in it, so the
 * form can underline those two as links. If a word is missing from the text it is simply not a
 * link; the text is always whole.
 */
export function consentParts(text: string, terms: string, privacy: string): ConsentPart[] {
  const marks = (
    [
      ["terms", terms],
      ["privacy", privacy],
    ] as const
  )
    .map(([page, word]) => ({ page: page as LegalPage, word, at: text.indexOf(word) }))
    .filter((m) => m.word !== "" && m.at >= 0)
    .sort((a, b) => a.at - b.at);
  const parts: ConsentPart[] = [];
  let from = 0;
  for (const m of marks) {
    if (m.at < from) continue; // overlapping marks: keep the earlier
    if (m.at > from) parts.push({ text: text.slice(from, m.at), page: null });
    parts.push({ text: m.word, page: m.page });
    from = m.at + m.word.length;
  }
  if (from < text.length) parts.push({ text: text.slice(from), page: null });
  return parts;
}

/** The catalogue keys of a legal page's paragraphs, in order. */
export const LEGAL_PARAGRAPHS: Record<LegalPage, readonly MessageKey[]> = {
  terms: [
    "player.legal.terms.1",
    "player.legal.terms.2",
    "player.legal.terms.3",
    "player.legal.terms.4",
  ],
  privacy: [
    "player.legal.privacy.1",
    "player.legal.privacy.2",
    "player.legal.privacy.3",
    "player.legal.privacy.4",
  ],
};
