import type { Page, Route } from "@playwright/test";
import type { AppError, SessionOwner, SignupChallenge, TotpStarted } from "@arena/ui/generated";
import { t } from "../../../packages/ui/src/messages/index.js";

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
  mfa_state: "pending",
  has_password: false,
  backup_codes_acked: false,
};
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
  start: { signedIn?: boolean; owner?: Partial<SessionOwner> } = {},
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
      default: {
        const next = cloud.nextAccount.shift();
        if (next) return fulfil(route, next);
        if (!cloud.signedIn) return fulfil(route, { status: 401, body: err("AUTH_FAILED") });
        return fulfil(route, account(cloud, url.pathname, body));
      }
    }
  });
  return cloud;
}

/** The cloud's account-setup rules, on `cloud.owner`. */
function account(cloud: Cloud, path: string, body: Record<string, string> | null): Reply {
  const o = cloud.owner;
  const enrolled = o.mfa_state === "enrolled";
  switch (path) {
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
