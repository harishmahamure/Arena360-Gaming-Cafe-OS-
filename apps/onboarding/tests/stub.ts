import type { Page, Route } from "@playwright/test";
import type {
  AppError,
  CafeDetails,
  Country,
  SessionOwner,
  SignupChallenge,
  TotpStarted,
  TrustedDevice,
  WizardBranch,
  WizardState,
  WizardStep,
} from "@arena/ui/generated";
import { t } from "../../../packages/ui/src/messages/index.js";
import { normalizeGstin, validGstin } from "../src/lib/gstin";

export { t };

// A stub of the cloud's `/v1` API for the onboarding pages, served with `page.route`.

export type Reply = {
  status: number;
  body: unknown;
  headers?: Record<string, string>;
  /** Answer only after this many ms. */
  delayMs?: number;
  /** Fail as a network error (offline) instead of answering. */
  abort?: boolean;
};

export const CHALLENGE: SignupChallenge = {
  challenge_id: "01JCHALLENGE00000000000000",
  expires_in_s: 300,
  resend_after_s: 30,
};
export const NEW_OWNER: SessionOwner = {
  name: "Asha",
  cafe_name: "Pixel Arena Gaming",
  city: "Pune",
  mfa_state: "pending",
  has_password: false,
  backup_codes_acked: false,
  sms_fallback: false,
};
export const ENROLLED_OWNER: Partial<SessionOwner> = {
  has_password: true,
  mfa_state: "enrolled",
  backup_codes_acked: true,
};
export const PASSWORD = "Tandoori-Nights-42";
export const SIGNIN_CHALLENGE = "01JSIGNIN0000000000000000000";
export const DEVICES: TrustedDevice[] = [
  {
    id: "dev-1",
    label: "Chrome on Android",
    last_seen_at: Date.UTC(2026, 9, 20, 9, 30),
    created_at: Date.UTC(2026, 9, 1, 9, 30),
    current: true,
  },
  {
    id: "dev-2",
    label: "Safari on iOS",
    last_seen_at: Date.UTC(2026, 9, 18, 18, 5),
    created_at: Date.UTC(2026, 9, 2, 9, 30),
    current: false,
  },
];
export const GOOD_CODE = "123456";
/** The authenticator code the stub accepts (for confirm and regenerate). */
export const GOOD_TOTP = "246810";
/** The stub's secrets, one per `start` (base32, 32 characters). */
export const SECRETS = [
  "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP",
  "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ",
  "MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43U",
];
export const QR_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#fff"/><rect x="2" y="2" width="6" height="6"/></svg>'; // i18n-ignore
export const CODES = Array.from({ length: 10 }, (_, i) => `ABCDE-FG${String(i).padStart(3, "0")}`);
export const NEW_CODES = Array.from(
  { length: 10 },
  (_, i) => `NEWCO-DE${String(i).padStart(3, "0")}`,
);

export const STEPS: WizardStep[] = [
  "cafe",
  "tier",
  "install_hub",
  "devices",
  "rates",
  "products",
  "staff",
  "printer_payments",
  "test_session",
  "go_live",
];
export const COUNTRIES: Country[] = [
  { code: "IN", name: "India", currency: "INR", timezone: "Asia/Kolkata", gst: true },
  { code: "NP", name: "Nepal", currency: "NPR", timezone: "Asia/Kathmandu", gst: false },
  { code: "AE", name: "United Arab Emirates", currency: "AED", timezone: "Asia/Dubai", gst: false },
  { code: "GB", name: "United Kingdom", currency: "GBP", timezone: "Europe/London", gst: false },
];
export const TIMEZONES = [
  "America/New_York",
  "Asia/Dubai",
  "Asia/Kathmandu",
  "Asia/Kolkata",
  "Europe/London",
  "UTC",
];
export const GSTIN = "27AAPFU0939F1ZV";
export const BRANCH: WizardBranch = {
  id: "01JBRANCH0000000000000000000",
  name: "Pixel Arena Gaming",
  city: "Pune",
  country: "IN",
  currency: "INR",
  timezone: "Asia/Kolkata",
  gst_registered: false,
  gstin: null,
  branch_code: "PAG",
  code_editable: true,
};
export const READY_OWNER: Partial<SessionOwner> = {
  has_password: true,
  mfa_state: "enrolled",
  backup_codes_acked: true,
};

export type WizardStart = { done?: WizardStep[]; branch?: Partial<WizardBranch> };

export const err = (code: AppError["code"], extra: Partial<AppError> = {}): AppError => ({
  code,
  message: "stub",
  ...extra,
});

export type Cloud = {
  starts: unknown[];
  resends: unknown[];
  verifies: { challenge_id: string; code: string }[];
  /** One-shot replies that replace the next answer of each endpoint. */
  nextStart: Reply[];
  nextResend: Reply[];
  nextVerify: Reply[];
  /** One-shot replies for `GET /v1/session` and `POST /v1/session/refresh`. */
  nextSession: Reply[];
  nextRefresh: Reply[];
  sessionChecks: number;
  refreshes: number;
  signedIn: boolean;
  /** The signed-in owner, as `GET /v1/session` shows them. */
  owner: SessionOwner;
  passwords: string[];
  /** How many times `totp/start` was called. */
  totpStarts: number;
  confirms: string[];
  regenerates: string[];
  acks: number;
  nextAccount: Reply[];
  signins: { login: string; password: string }[];
  nextSignin: Reply[];
  smsSends: unknown[];
  nextSms: Reply[];
  signinVerifies: {
    challenge_id: string;
    method: string;
    code: string;
    trust_device: boolean;
  }[];
  nextSigninVerify: Reply[];
  trusted: boolean;
  usedBackupCodes: string[];
  backupRemaining: number;
  devices: TrustedDevice[];
  smsFallbackSets: boolean[];
  removedDevices: string[];
  signOuts: number;
  done: Set<WizardStep>;
  tenant: { id: string; name: string } | null;
  branch: WizardBranch | null;
  cafeSaves: CafeDetails[];
  codeSets: { id: string; code: string }[];
  takenCodes: string[];
  wizardLoads: number;
  nextWizard: Reply[];
};

async function fulfil(route: Route, r: Reply) {
  if (r.delayMs) await new Promise((done) => setTimeout(done, r.delayMs));
  if (r.abort) return route.abort("internetdisconnected");
  if (r.status === 204) return route.fulfill({ status: 204, headers: r.headers });
  return route.fulfill({
    status: r.status,
    contentType: "application/json",
    headers: r.headers,
    body: JSON.stringify(r.body),
  });
}

const NO_CONTENT: Reply = { status: 204, body: null };

/**
 * Serves `/v1/*`: the correct sign-up code is {@link GOOD_CODE} (anything else is
 * `OTP_INVALID`); account setup follows the cloud's rules on `cloud.owner`, with
 * {@link GOOD_TOTP} as the only right authenticator code. `owner` starts as given (signed in
 * when `signedIn`).
 */
export async function stubCloud(
  page: Page,
  start: { signedIn?: boolean; owner?: Partial<SessionOwner>; wizard?: WizardStart } = {},
): Promise<Cloud> {
  const cloud: Cloud = {
    starts: [],
    resends: [],
    verifies: [],
    nextStart: [],
    nextResend: [],
    nextVerify: [],
    nextSession: [],
    nextRefresh: [],
    sessionChecks: 0,
    refreshes: 0,
    signedIn: start.signedIn ?? false,
    owner: { ...NEW_OWNER, ...start.owner },
    passwords: [],
    totpStarts: 0,
    confirms: [],
    regenerates: [],
    acks: 0,
    nextAccount: [],
    signins: [],
    nextSignin: [],
    smsSends: [],
    nextSms: [],
    signinVerifies: [],
    nextSigninVerify: [],
    trusted: false,
    usedBackupCodes: [],
    backupRemaining: 9,
    devices: DEVICES.map((d) => ({ ...d })),
    smsFallbackSets: [],
    removedDevices: [],
    signOuts: 0,
    done: new Set(start.wizard?.done ?? (start.wizard?.branch ? ["cafe"] : [])),
    tenant: start.wizard?.branch
      ? { id: "01JTENANT0000000000000000000", name: start.wizard.branch.name ?? BRANCH.name }
      : null,
    branch: start.wizard?.branch ? { ...BRANCH, ...start.wizard.branch } : null,
    cafeSaves: [],
    codeSets: [],
    takenCodes: ["SEC"],
    wizardLoads: 0,
    nextWizard: [],
  };
  await page.route("**/v1/**", async (route) => {
    const url = new URL(route.request().url());
    const body = route.request().postDataJSON();
    switch (url.pathname) {
      case "/v1/signup/start":
        cloud.starts.push(body);
        return fulfil(route, cloud.nextStart.shift() ?? { status: 202, body: CHALLENGE });
      case "/v1/signup/resend":
        cloud.resends.push(body);
        return fulfil(route, cloud.nextResend.shift() ?? { status: 202, body: CHALLENGE });
      case "/v1/signup/verify": {
        cloud.verifies.push(body);
        const next = cloud.nextVerify.shift();
        if (next) return fulfil(route, next);
        if (body.code === GOOD_CODE) {
          cloud.signedIn = true;
          return fulfil(route, { status: 200, body: { next: "account_setup" } });
        }
        return fulfil(route, { status: 400, body: err("OTP_INVALID") });
      }
      case "/v1/session/refresh": {
        cloud.refreshes += 1;
        const next = cloud.nextRefresh.shift();
        if (next) return fulfil(route, next);
        return cloud.signedIn
          ? fulfil(route, { status: 200, body: { owner: cloud.owner } })
          : fulfil(route, { status: 401, body: err("AUTH_FAILED") });
      }
      case "/v1/session": {
        cloud.sessionChecks += 1;
        const next = cloud.nextSession.shift();
        if (next) return fulfil(route, next);
        return cloud.signedIn
          ? fulfil(route, { status: 200, body: { owner: cloud.owner } })
          : fulfil(route, { status: 401, body: err("AUTH_FAILED") });
      }
      case "/v1/signin": {
        cloud.signins.push(body);
        const next = cloud.nextSignin.shift();
        if (next) return fulfil(route, next);
        if (body.password !== PASSWORD) {
          return fulfil(route, { status: 401, body: err("AUTH_FAILED") });
        }
        if (cloud.owner.mfa_state !== "enrolled") {
          cloud.signedIn = true;
          return fulfil(route, { status: 200, body: { next: "account_setup" } });
        }
        if (cloud.trusted) {
          cloud.signedIn = true;
          return fulfil(route, { status: 200, body: { next: "done" } });
        }
        const methods = ["totp", "backup_code", ...(cloud.owner.sms_fallback ? ["sms"] : [])];
        return fulfil(route, {
          status: 200,
          body: { challenge_id: SIGNIN_CHALLENGE, methods },
        });
      }
      case "/v1/signin/sms": {
        cloud.smsSends.push(body);
        const next = cloud.nextSms.shift();
        if (next) return fulfil(route, next);
        if (!cloud.owner.sms_fallback)
          return fulfil(route, { status: 403, body: err("FORBIDDEN") });
        return fulfil(route, { status: 202, body: { resend_after_s: 30 } });
      }
      case "/v1/signin/verify": {
        cloud.signinVerifies.push(body);
        const next = cloud.nextSigninVerify.shift();
        if (next) return fulfil(route, next);
        const normalized = CODES.map((c) => c.replace("-", ""));
        let ok = false;
        if (body.method === "totp") ok = body.code === GOOD_TOTP;
        if (body.method === "sms") ok = body.code === GOOD_CODE;
        if (body.method === "backup_code") {
          ok = normalized.includes(body.code) && !cloud.usedBackupCodes.includes(body.code);
          if (ok) cloud.usedBackupCodes.push(body.code);
        }
        if (!ok) return fulfil(route, { status: 401, body: err("AUTH_FAILED") });
        cloud.signedIn = true;
        if (body.trust_device) cloud.trusted = true;
        const reply =
          body.method === "backup_code"
            ? { next: "done", backup_codes_remaining: cloud.backupRemaining }
            : { next: "done" };
        return fulfil(route, { status: 200, body: reply });
      }
      case "/v1/meta/countries":
        return fulfil(route, { status: 200, body: COUNTRIES });
      case "/v1/meta/timezones":
        return fulfil(route, { status: 200, body: TIMEZONES });
      default: {
        if (url.pathname.startsWith("/v1/wizard") || url.pathname.startsWith("/v1/branches/")) {
          if (url.pathname === "/v1/wizard") cloud.wizardLoads += 1;
          const next = cloud.nextWizard.shift();
          if (next) return fulfil(route, next);
          return fulfil(route, wizardRoute(cloud, url.pathname, body, route.request().method()));
        }
        const next = cloud.nextAccount.shift();
        if (next) return fulfil(route, next);
        if (!cloud.signedIn) return fulfil(route, { status: 401, body: err("AUTH_FAILED") });
        return fulfil(route, account(cloud, url.pathname, body, route.request().method()));
      }
    }
  });
  return cloud;
}

export function wizardBody(cloud: Cloud): WizardState {
  const current = STEPS.find((s) => !cloud.done.has(s)) ?? "go_live";
  return {
    current,
    steps: STEPS.map((key) => ({
      key,
      state: cloud.done.has(key) ? "done" : "pending",
      can_skip: key === "products" || key === "staff" || key === "printer_payments",
    })),
    ...(cloud.tenant ? { tenant: cloud.tenant } : {}),
    ...(cloud.branch ? { branch: cloud.branch } : {}),
  };
}

function initials(name: string): string {
  const words = name
    .toUpperCase()
    .split(/[^A-Z0-9]+/)
    .filter(Boolean);
  let code =
    words.length > 1
      ? words
          .slice(0, 6)
          .map((w) => w[0])
          .join("")
      : "";
  if (code.length < 3) code = words.join("").slice(0, 3);
  return code.padEnd(3, "X");
}

function wizardRoute(cloud: Cloud, path: string, body: unknown, method: string): Reply {
  if (!cloud.signedIn) return { status: 401, body: err("AUTH_FAILED") };
  const o = cloud.owner;
  if (!o.has_password || o.mfa_state !== "enrolled" || !o.backup_codes_acked) {
    return { status: 403, body: err("SETUP_INCOMPLETE") };
  }
  if (path === "/v1/wizard" && method === "GET") return { status: 200, body: wizardBody(cloud) };
  if (path === "/v1/wizard/cafe" && method === "PUT") {
    const d = body as CafeDetails;
    cloud.cafeSaves.push(d);
    const invalid = (field: string) => ({
      status: 422,
      body: err("VALIDATION_FAILED", { field }),
    });
    if (!d.cafe_name?.trim()) return invalid("cafe_name");
    if (!d.city?.trim()) return invalid("city");
    if (!TIMEZONES.includes(d.timezone)) return invalid("timezone");
    const gstin = d.gstin ? normalizeGstin(d.gstin) : "";
    if (d.gst_registered && !validGstin(gstin)) return invalid("gstin");
    if (!d.gst_registered && gstin) return invalid("gstin");
    const fields = {
      name: d.cafe_name.trim(),
      city: d.city.trim(),
      country: d.country,
      currency: d.currency,
      timezone: d.timezone,
      gst_registered: d.gst_registered,
      gstin: d.gst_registered ? gstin : null,
    };
    if (cloud.branch) {
      cloud.branch = { ...cloud.branch, ...fields };
      cloud.tenant = { id: cloud.tenant?.id ?? "01JTENANT0000000000000000000", name: fields.name };
    } else {
      cloud.branch = { ...BRANCH, ...fields, branch_code: initials(fields.name) };
      cloud.tenant = { id: "01JTENANT0000000000000000000", name: fields.name };
    }
    cloud.done.add("cafe");
    return { status: 200, body: wizardBody(cloud) };
  }
  const m = /^\/v1\/branches\/([^/]+)\/code$/.exec(path);
  if (m && method === "PUT") {
    const id = decodeURIComponent(m[1]);
    const code = String((body as { code?: string } | null)?.code ?? "")
      .trim()
      .toUpperCase();
    cloud.codeSets.push({ id, code });
    if (!cloud.branch || cloud.branch.id !== id) return { status: 403, body: err("FORBIDDEN") };
    if (!cloud.branch.code_editable) return { status: 409, body: err("ALREADY_SET") };
    if (!/^[A-Z0-9]{3,6}$/.test(code)) {
      return { status: 422, body: err("VALIDATION_FAILED", { field: "code" }) };
    }
    if (cloud.takenCodes.includes(code)) {
      return { status: 409, body: err("BRANCH_CODE_TAKEN", { field: "code" }) };
    }
    cloud.branch.branch_code = code;
    return { status: 200, body: { branch_code: code } };
  }
  return { status: 404, body: err("NOT_FOUND") };
}

/** The cloud's account-setup rules, on `cloud.owner`. */
function account(
  cloud: Cloud,
  path: string,
  body: Record<string, string> | null,
  method: string,
): Reply {
  const o = cloud.owner;
  const enrolled = o.mfa_state === "enrolled";
  if (path.startsWith("/v1/account/devices/") && method === "DELETE") {
    if (!enrolled) return { status: 403, body: err("SETUP_INCOMPLETE") };
    const id = decodeURIComponent(path.slice("/v1/account/devices/".length));
    cloud.removedDevices.push(id);
    const before = cloud.devices.length;
    cloud.devices = cloud.devices.filter((d) => d.id !== id);
    return cloud.devices.length < before ? NO_CONTENT : { status: 404, body: err("NOT_FOUND") };
  }
  switch (path) {
    case "/v1/account/security/sms-fallback": {
      if (!enrolled) return { status: 403, body: err("SETUP_INCOMPLETE") };
      const enabled = (body as unknown as { enabled: boolean } | null)?.enabled === true;
      cloud.smsFallbackSets.push(enabled);
      o.sms_fallback = enabled;
      return NO_CONTENT;
    }
    case "/v1/account/devices":
      if (!enrolled) return { status: 403, body: err("SETUP_INCOMPLETE") };
      return { status: 200, body: cloud.devices };
    case "/v1/account/sign-out-everywhere":
      if (!enrolled) return { status: 403, body: err("SETUP_INCOMPLETE") };
      cloud.signOuts += 1;
      cloud.signedIn = false;
      cloud.trusted = false;
      cloud.devices = [];
      return NO_CONTENT;
    case "/v1/account/password": {
      const pw = body?.password ?? "";
      cloud.passwords.push(pw);
      if (o.has_password) return { status: 409, body: err("ALREADY_SET") };
      if ([...pw].length < 10)
        return { status: 422, body: err("PASSWORD_TOO_SHORT", { field: "password" }) };
      if (pw.toLowerCase() === "password123")
        return { status: 422, body: err("PASSWORD_COMMON", { field: "password" }) };
      o.has_password = true;
      return NO_CONTENT;
    }
    case "/v1/account/mfa/totp/start": {
      if (!o.has_password) return { status: 403, body: err("PASSWORD_REQUIRED") };
      if (enrolled) return { status: 409, body: err("ALREADY_SET") };
      const secret = SECRETS[cloud.totpStarts % SECRETS.length];
      cloud.totpStarts += 1;
      const started: TotpStarted = {
        secret,
        otpauth_uri: `otpauth://totp/Arena%20360:%2B919876543210?secret=${secret}`,
        qr_svg: QR_SVG,
      };
      return { status: 200, body: started };
    }
    case "/v1/account/mfa/totp/confirm": {
      cloud.confirms.push(body?.code ?? "");
      if (enrolled) return { status: 409, body: err("ALREADY_SET") };
      if (body?.code !== GOOD_TOTP) return { status: 400, body: err("TOTP_INVALID") };
      o.mfa_state = "enrolled";
      return { status: 200, body: { backup_codes: CODES } };
    }
    case "/v1/account/mfa/backup-codes/ack":
      if (!enrolled) return { status: 403, body: err("FORBIDDEN") };
      cloud.acks += 1;
      o.backup_codes_acked = true;
      return NO_CONTENT;
    case "/v1/account/mfa/backup-codes/regenerate":
      cloud.regenerates.push(body?.code ?? "");
      if (!enrolled) return { status: 403, body: err("FORBIDDEN") };
      if (body?.code !== GOOD_TOTP) return { status: 400, body: err("TOTP_INVALID") };
      o.backup_codes_acked = false;
      return { status: 200, body: { backup_codes: NEW_CODES } };
    default:
      return { status: 404, body: err("NOT_FOUND") };
  }
}

/** Fills the "Get started" form with valid details. */
export async function fillForm(page: Page) {
  await page.getByLabel(t("onboarding.field.name"), { exact: true }).fill("Asha Rao");
  await page.getByLabel(t("onboarding.field.phone"), { exact: true }).fill("98765 43210");
  await page.getByLabel(t("onboarding.field.email"), { exact: true }).fill("asha@example.in");
  await page.getByLabel(t("onboarding.field.cafe_name"), { exact: true }).fill("Pixel Den");
  await page.getByLabel(t("onboarding.field.city"), { exact: true }).fill("Pune");
}

/** Opens `/`, fills the form and sends it, landing on the code step. */
export async function toCodeStep(page: Page) {
  await page.goto("/");
  await fillForm(page);
  await page.getByRole("button", { name: t("onboarding.start.send") }).click();
  await page.getByRole("heading", { level: 1, name: t("onboarding.code.title") }).waitFor();
}
