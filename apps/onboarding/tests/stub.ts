import type { Page, Route } from "@playwright/test";
import type { AppError, SessionInfo, SignupChallenge } from "@arena/ui/generated";
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
export const SESSION: SessionInfo = { owner: { name: "Asha", mfa_state: "pending" } };
export const GOOD_CODE = "123456";

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
};

async function fulfil(route: Route, r: Reply) {
  if (r.delayMs) await new Promise((done) => setTimeout(done, r.delayMs));
  if (r.abort) return route.abort("internetdisconnected");
  return route.fulfill({
    status: r.status,
    contentType: "application/json",
    headers: r.headers,
    body: JSON.stringify(r.body),
  });
}

/** Serves `/v1/*`: the correct code is {@link GOOD_CODE}; anything else is `OTP_INVALID`. */
export async function stubCloud(page: Page): Promise<Cloud> {
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
    signedIn: false,
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
          ? fulfil(route, { status: 200, body: SESSION })
          : fulfil(route, { status: 401, body: err("AUTH_FAILED") });
      }
      case "/v1/session": {
        cloud.sessionChecks += 1;
        const next = cloud.nextSession.shift();
        if (next) return fulfil(route, next);
        return cloud.signedIn
          ? fulfil(route, { status: 200, body: SESSION })
          : fulfil(route, { status: 401, body: err("AUTH_FAILED") });
      }
      default:
        return fulfil(route, { status: 404, body: err("NOT_FOUND") });
    }
  });
  return cloud;
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
