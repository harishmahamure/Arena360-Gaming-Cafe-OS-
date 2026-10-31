import type { Page, Route } from "@playwright/test";
import type {
  AddOns,
  AppError,
  Band,
  CafeDetails,
  Country,
  OwnerBranch,
  SessionOwner,
  SetupCommandView,
  GoLiveView,
  SetupDevicesView,
  SetupStartView,
  SignupChallenge,
  StaffInvite,
  StaffInviteView,
  SubscriptionCheckout,
  SubscriptionQuote,
  SubscriptionStatus,
  Tier,
  TierChoices,
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

export type WizardStart = {
  done?: WizardStep[];
  branch?: Partial<WizardBranch>;
  tier?: Tier;
  otherFree?: boolean;
  hubPaired?: boolean;
  setupCode?: string;
  skipped?: WizardStep[];
  invites?: StaffInviteView[];
  devices?: Partial<SetupDevicesView>;
  goLive?: Partial<GoLiveView>;
  start?: Partial<SetupStartView>;
  /** Billing on (Story 10.4): the branch's band, and whether that band is on sale. */
  billing?: { band?: Band | null; onSale?: boolean };
};

/** What the stub charges, in minor units of INR (the seed price book). */
export const BOOK = {
  basic: { monthly: 20_000, yearly: 200_000 },
  premium: { monthly: 199_900, yearly: 1_999_000 },
  devices_20: { monthly: 5_000, yearly: 50_000 },
  tunnel: { monthly: 20_000, yearly: 200_000 },
};
/** The renewal date the stub's quote answers (2027-01-15 08:00 UTC). */
export const RENEWS_AT = Date.UTC(2027, 0, 15, 8);

export type SubscribeBody = {
  tier: Tier;
  cycle: "monthly" | "yearly";
  add_ons: AddOns;
  billing_country: string;
};

export const STAFF_CODES = ["K7Q49MZD", "Q4K79MZD", "M3XT8PRW"];

export const PAIR_CODES = ["7K2M9PQR", "HX4TW8NB", "3FJD6YZC", "QW5RS2KV"];
export const PAIR_TTL_MS = 15 * 60 * 1000;

export const TIER_LIMITS: Record<Tier, { devices: number; users: number; branches: number }> = {
  free: { devices: 20, users: 3, branches: 1 },
  basic: { devices: 60, users: 10, branches: 1 },
  premium: { devices: 200, users: 50, branches: 1 },
};

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
  tier: Tier | null;
  otherFree: boolean;
  tierPicks: Tier[];
  tierBodies: unknown[];
  hubPaired: boolean;
  setupCode: string | null;
  setupReveals: number;
  pairGets: number;
  pairCodes: { code: string; expires_at: number }[];
  clockMs: number | null;
  skipped: Set<WizardStep>;
  invites: StaffInviteView[];
  invitePosts: StaffInvite[];
  deviceView: SetupDevicesView;
  devicePosts: { kind: string; payload: unknown }[];
  goLive: GoLiveView;
  goLivePosts: number;
  reissued: string[];
  setupStart: SetupStartView;
  setupStartCalls: string[];
  /** Billing on: the band, and whether it is on sale. `null` while billing is off. */
  billing: { band: Band | undefined; onSale: boolean } | null;
  quoteBodies: unknown[];
  subscribeBodies: SubscribeBody[];
  /** Ids of the subscriptions the stub created, and the one still unpaid. */
  subscriptionIds: string[];
  pending: { id: string; body: SubscribeBody } | null;
  /** Razorpay subscriptions cancelled because the owner chose something else. */
  cancelled: string[];
  nextQuote: Reply[];
  nextSubscribe: Reply[];
  /** Story 10.5: what `GET /branches/:id/subscription` answers (`null`: 404). */
  existing: SubscriptionStatus | null;
  /** The owner's branches, as `GET /branches` lists them. */
  branchList: OwnerBranch[];
  /** A branch id the signed-in owner does not own: every request naming it is `FORBIDDEN`. */
  forbiddenBranch: string | null;
  /** Serve `/v1/wizard/events` as a stream that wakes the wizard on every (re)connect. */
  sse: boolean;
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
    tier: start.wizard?.tier ?? (start.wizard?.done?.includes("tier") ? "free" : null),
    otherFree: start.wizard?.otherFree ?? false,
    tierPicks: [],
    tierBodies: [],
    hubPaired: start.wizard?.hubPaired ?? false,
    setupCode: start.wizard?.setupCode ?? null,
    setupReveals: 0,
    pairGets: 0,
    pairCodes: [],
    clockMs: null,
    skipped: new Set(start.wizard?.skipped ?? []),
    invites: start.wizard?.invites ?? [],
    invitePosts: [],
    deviceView: {
      setup: null,
      reported_at: null,
      now: Date.now(),
      hub_online: true,
      commands: [],
      ...start.wizard?.devices,
    },
    devicePosts: [],
    goLive: {
      items: [
        { key: "opening_hours", done: true },
        { key: "backup_location", done: true },
        { key: "standby", done: false },
        { key: "ad_consent", done: true },
      ],
      free: true,
      live: false,
      ...start.wizard?.goLive,
    },
    goLivePosts: 0,
    reissued: [],
    setupStart: {
      other_branches: 0,
      sources: [],
      enabled: false,
      selected: null,
      status: null,
      ...start.wizard?.start,
    },
    setupStartCalls: [],
    billing: start.wizard?.billing
      ? {
          band:
            start.wizard.billing.band === null ? undefined : (start.wizard.billing.band ?? "IN"),
          onSale: start.wizard.billing.onSale ?? true,
        }
      : null,
    quoteBodies: [],
    subscribeBodies: [],
    subscriptionIds: [],
    pending: null,
    cancelled: [],
    nextQuote: [],
    nextSubscribe: [],
    existing: null,
    branchList: [],
    forbiddenBranch: null,
    sse: false,
  };
  await page.route("**/v1/**", async (route) => {
    const url = new URL(route.request().url());
    const body = route.request().postDataJSON();
    switch (url.pathname) {
      case "/v1/wizard/events":
        if (cloud.sse) {
          return route.fulfill({
            status: 200,
            contentType: "text/event-stream",
            body: "retry: 200\n\nevent: wizard.changed\ndata: {}\n\n",
          });
        }
        return route.abort("failed");
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
        const named = url.searchParams.get("branch_id");
        if (
          cloud.forbiddenBranch &&
          (named === cloud.forbiddenBranch ||
            url.pathname.startsWith(`/v1/branches/${cloud.forbiddenBranch}/`))
        ) {
          return fulfil(route, { status: 403, body: err("FORBIDDEN") });
        }
        if (
          url.pathname.startsWith("/v1/wizard") ||
          url.pathname.startsWith("/v1/branches/") ||
          url.pathname === "/v1/branches"
        ) {
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
  const current = STEPS.find((s) => !cloud.done.has(s) && !cloud.skipped.has(s)) ?? "go_live";
  return {
    current,
    steps: STEPS.map((key) => ({
      key,
      state: cloud.done.has(key) ? "done" : cloud.skipped.has(key) ? "skipped" : "pending",
      can_skip: key === "products" || key === "staff" || key === "printer_payments",
    })),
    ...(cloud.tenant ? { tenant: cloud.tenant } : {}),
    ...(cloud.branch ? { branch: cloud.branch } : {}),
  };
}

export function tierChoices(cloud: Cloud): TierChoices {
  const keys: Tier[] = ["free", "basic", "premium"];
  const billing = cloud.billing;
  return {
    tiers: keys.map((key) => {
      const paidReason = !billing
        ? "billing_disabled"
        : !billing.band || !billing.onSale
          ? "band_not_available"
          : undefined;
      const reason =
        key === "free" ? (cloud.otherFree ? "one_free_branch" : undefined) : paidReason;
      return {
        key,
        available: reason === undefined,
        ...(reason ? { reason } : {}),
        limits: TIER_LIMITS[key],
      } as TierChoices["tiers"][number];
    }),
    billing_enabled: billing !== null,
    ...(billing?.band ? { band: billing.band } : {}),
    ...(cloud.tier ? { current: cloud.tier } : {}),
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
  if (path === "/v1/wizard/tier") {
    if (!cloud.branch) return { status: 403, body: err("FORBIDDEN") };
    if (method === "GET") return { status: 200, body: tierChoices(cloud) };
    if (method === "PUT") {
      cloud.tierBodies.push(body);
      const tier = (body as { tier?: Tier } | null)?.tier;
      if (tier !== "free" && tier !== "basic" && tier !== "premium") {
        return { status: 400, body: err("BAD_REQUEST") };
      }
      cloud.tierPicks.push(tier);
      if (tier !== "free" && cloud.billing) {
        return { status: 422, body: err("VALIDATION_FAILED", { field: "tier" }) };
      }
      if (tier !== "free") return { status: 409, body: err("BILLING_DISABLED", { field: "tier" }) };
      if (cloud.tier && cloud.tier !== "free") return { status: 409, body: err("ALREADY_SET") };
      if (cloud.otherFree) return { status: 409, body: err("ONE_FREE_BRANCH", { field: "tier" }) };
      cloud.tier = "free";
      cloud.done.add("tier");
      return { status: 200, body: wizardBody(cloud) };
    }
  }
  if (path === "/v1/wizard/install-hub" || path === "/v1/wizard/install-hub/code") {
    if (!cloud.branch) return { status: 403, body: err("FORBIDDEN") };
    if (!cloud.done.has("cafe") || !cloud.done.has("tier")) {
      return { status: 409, body: err("STEP_NOT_READY") };
    }
    const now = cloud.clockMs ?? Date.now();
    if (path === "/v1/wizard/install-hub" && method === "GET") {
      cloud.pairGets += 1;
      const live = cloud.pairCodes.at(-1);
      return {
        status: 200,
        body: {
          active: live && live.expires_at > now ? { expires_at: live.expires_at } : null,
          hub_paired: cloud.hubPaired,
          tier: cloud.tier ?? "free",
          server_now: now,
        },
      };
    }
    if (path === "/v1/wizard/install-hub/code" && method === "POST") {
      const issued = {
        code: PAIR_CODES[cloud.pairCodes.length % PAIR_CODES.length],
        expires_at: now + PAIR_TTL_MS,
        server_now: now,
      };
      cloud.pairCodes.push(issued);
      return { status: 200, body: issued };
    }
  }
  if (path === "/v1/wizard/devices") {
    if (!cloud.hubPaired) return { status: 409, body: err("STEP_NOT_READY") };
    if (method === "GET") return { status: 200, body: cloud.deviceView };
    const b = body as { kind: string; payload: unknown };
    cloud.devicePosts.push(b);
    const command: SetupCommandView = {
      key: `01JCOMMAND${String(cloud.devicePosts.length).padStart(16, "0")}`,
      kind: b.kind,
      payload: b.payload,
      status: "pending",
      error: null,
    };
    cloud.deviceView.commands.push(command);
    return { status: 200, body: command };
  }
  const buying = /^\/v1\/branches\/([^/]+)\/subscription(\/quote)?$/.exec(path);
  if (buying && method === "POST") return subscription(cloud, buying[2] !== undefined, body);
  if (path === "/v1/branches" && method === "GET") return { status: 200, body: cloud.branchList };
  if (path.startsWith("/v1/branches/") && path.endsWith("/subscription") && method === "GET") {
    return cloud.existing
      ? { status: 200, body: cloud.existing }
      : { status: 404, body: err("NOT_FOUND") };
  }
  const starting = /^\/v1\/branches\/([^/]+)\/setup-start(\/retry)?$/.exec(path);
  if (starting) {
    const v = cloud.setupStart;
    if (starting[2]) {
      cloud.setupStartCalls.push("retry");
      if (v.status === "failed") v.status = "retry";
    } else if (method === "PUT") {
      const b = body as { enabled: boolean; source_branch_id?: string };
      cloud.setupStartCalls.push(JSON.stringify(b));
      v.enabled = b.enabled;
      v.selected = b.enabled ? (b.source_branch_id ?? v.sources[0]?.branch_id ?? null) : null;
      v.status = null;
    }
    return { status: 200, body: v };
  }
  if (path === "/v1/wizard/go-live") {
    if (method === "GET") return { status: 200, body: cloud.goLive };
    cloud.goLivePosts += 1;
    cloud.goLive.live = true;
    cloud.done.add("go_live");
    return { status: 200, body: wizardBody(cloud) };
  }
  const skipping = /^\/v1\/wizard\/(products|printer-payments)\/skip$/.exec(path);
  if (skipping && method === "POST") {
    if (!cloud.hubPaired) return { status: 409, body: err("STEP_NOT_READY") };
    cloud.skipped.add(skipping[1] === "products" ? "products" : "printer_payments");
    return { status: 200, body: wizardBody(cloud) };
  }
  if (path.startsWith("/v1/wizard/staff")) {
    if (!cloud.hubPaired) return { status: 409, body: err("STEP_NOT_READY") };
    if (path === "/v1/wizard/staff" && method === "GET") {
      return { status: 200, body: { invites: cloud.invites } };
    }
    if (path === "/v1/wizard/staff" && method === "POST") {
      const b = body as StaffInvite;
      cloud.invitePosts.push(b);
      const invite: StaffInviteView = {
        id: `01JINVITE${String(cloud.invites.length).padStart(16, "0")}`,
        name: b.name,
        phone: b.phone,
        role: b.role,
        status: "waiting",
        error: null,
        setup_expires_at: Date.now() + 72 * 3_600_000,
      };
      cloud.invites.push(invite);
      return { status: 200, body: { invite, code: STAFF_CODES[(cloud.invites.length - 1) % 3] } };
    }
    if (path === "/v1/wizard/staff/skip" && method === "POST") {
      cloud.skipped.add("staff");
      return { status: 200, body: wizardBody(cloud) };
    }
    const reissue = /^\/v1\/wizard\/staff\/([^/]+)\/code$/.exec(path);
    const invite = cloud.invites.find((i) => i.id === reissue?.[1]);
    if (reissue && method === "POST" && invite) {
      cloud.reissued.push(invite.id);
      invite.status = "waiting";
      invite.error = null;
      return { status: 200, body: { invite, code: STAFF_CODES[(cloud.reissued.length + 1) % 3] } };
    }
    return { status: 404, body: err("NOT_FOUND") };
  }
  if (path === "/v1/wizard/setup-code/reveal" && method === "POST") {
    cloud.setupReveals += 1;
    if (!cloud.done.has("install_hub")) return { status: 409, body: err("STEP_NOT_READY") };
    const code = cloud.setupCode;
    cloud.setupCode = null;
    return code ? { status: 200, body: { code } } : { status: 404, body: err("NOT_FOUND") };
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

const same = (a: SubscribeBody, b: SubscribeBody) =>
  a.tier === b.tier &&
  a.cycle === b.cycle &&
  a.add_ons.devices_20 === b.add_ons.devices_20 &&
  a.add_ons.tunnel === b.add_ons.tunnel;

function priced(cloud: Cloud, tier: Tier, cycle: "monthly" | "yearly", add_ons: AddOns) {
  const item = tier === "premium" ? BOOK.premium : BOOK.basic;
  const amount_minor =
    item[cycle] + add_ons.devices_20 * BOOK.devices_20[cycle] + add_ons.tunnel * BOOK.tunnel[cycle];
  const tax_minor = Math.round((amount_minor * 18) / 100);
  return {
    amount_minor,
    tax_minor,
    total_minor: amount_minor + tax_minor,
    band: cloud.billing?.band,
  };
}

/** `POST /v1/branches/:id/subscription` and its `/quote`, as the cloud answers them. */
function subscription(cloud: Cloud, quoting: boolean, body: unknown): Reply {
  const next = quoting ? cloud.nextQuote.shift() : cloud.nextSubscribe.shift();
  if (quoting) cloud.quoteBodies.push(body);
  if (next) return next;
  const b = body as SubscribeBody;
  const billing = cloud.billing;
  if (!billing) return { status: 409, body: err("BILLING_DISABLED", { field: "tier" }) };
  if (!billing.band || !billing.onSale) {
    return { status: 409, body: err("BAND_NOT_AVAILABLE", { field: "billing_country" }) };
  }
  const add_ons = { devices_20: 0, tunnel: 0, owner_login: 0, ...b.add_ons };
  const money = priced(cloud, b.tier, b.cycle, add_ons);
  const methods =
    money.total_minor > 1_500_000
      ? (["card", "emandate"] as const)
      : (["upi", "card", "emandate"] as const);
  if (quoting) {
    const quote: SubscriptionQuote = {
      band: billing.band,
      currency: "INR",
      tier: b.tier,
      cycle: b.cycle,
      add_ons,
      amount_minor: money.amount_minor,
      tax_minor: money.tax_minor,
      total_minor: money.total_minor,
      gst_percent: 18,
      renews_at: RENEWS_AT,
      methods: [...methods],
      ...(money.total_minor > 1_500_000
        ? { notice: "Renewals over Rs 15,000 need a card or bank mandate" }
        : {}),
    };
    return { status: 200, body: quote };
  }
  cloud.subscribeBodies.push(b);
  // The same unpaid choice answers the same Checkout; another cancels it and starts anew.
  if (!cloud.pending || !same(cloud.pending.body, { ...b, add_ons })) {
    if (cloud.pending) cloud.cancelled.push(cloud.pending.id);
    const id = `sub_stub_${String(cloud.subscriptionIds.length + 1).padStart(3, "0")}`;
    cloud.subscriptionIds.push(id);
    cloud.pending = { id, body: { ...b, add_ons } };
  }
  const checkout: SubscriptionCheckout = {
    subscription_id: `sb_${cloud.pending.id}`,
    state: "pending",
    key_id: "rzp_test_stub",
    razorpay_subscription_id: cloud.pending.id,
    band: billing.band,
    currency: "INR",
    tier: b.tier,
    cycle: b.cycle,
    methods: [...methods],
    amount_minor: money.amount_minor,
    tax_minor: money.tax_minor,
    total_minor: money.total_minor,
  };
  return { status: 201, body: checkout };
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
