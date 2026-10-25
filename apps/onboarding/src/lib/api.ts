// The cloud API client: `${PUBLIC_API_BASE}/v1/...` with the session cookies. Every failure
// comes back as the flat `AppError` envelope; a network failure or a body that is not one is
// reported as `INTERNAL` with `offline` set.

import { PUBLIC_API_BASE } from "$app/env/public";
import { goto } from "$app/navigation";
import type {
  AppError,
  BackupCodes,
  BranchCodeSet,
  CafeDetails,
  Country,
  ErrorCode,
  InstallHubState,
  PairingCodeIssued,
  SessionInfo,
  SetupCodeReveal,
  StaffInvite,
  StaffInvited,
  StaffInvites,
  SignupChallenge,
  SignupStart,
  SignupVerified,
  SigninMethod,
  SigninReply,
  SigninSmsSent,
  SigninVerified,
  Tier,
  TierChoices,
  TotpStarted,
  TrustedDevice,
  WizardState,
} from "@arena/ui/generated";
import { clearWizard } from "./wizard.svelte";

/** The API origin (empty: same origin, through the dev/preview proxy). */
const base = () => (PUBLIC_API_BASE ?? "").replace(/\/$/, "");

export type ApiFailure = {
  ok: false;
  status: number;
  error: AppError;
  /** Seconds from `Retry-After` (`RATE_LIMITED`), when sent. */
  retryAfterS: number | null;
  /** The API could not be reached at all. */
  offline: boolean;
};
export type ApiResult<T> = { ok: true; status: number; data: T } | ApiFailure;

const CODES: ReadonlySet<string> = new Set<ErrorCode>([
  "VALIDATION_FAILED",
  "RATE_LIMITED",
  "OTP_INVALID",
  "OTP_EXPIRED",
  "AUTH_FAILED",
  "FORBIDDEN",
  "NOT_FOUND",
  "BAD_REQUEST",
  "INTERNAL",
  "PASSWORD_TOO_SHORT",
  "PASSWORD_COMMON",
  "ALREADY_SET",
  "PASSWORD_REQUIRED",
  "TOTP_INVALID",
  "SETUP_INCOMPLETE",
  "AUTH_LOCKED",
  "BRANCH_CODE_TAKEN",
  "BILLING_DISABLED",
  "ONE_FREE_BRANCH",
  "STEP_NOT_READY",
]);

function isAppError(v: unknown): v is AppError {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return typeof o.code === "string" && typeof o.message === "string";
}

function internal(status: number, offline: boolean): ApiFailure {
  return {
    ok: false,
    status,
    error: { code: "INTERNAL", message: "" },
    retryAfterS: null,
    offline,
  };
}

/** Seconds in a `Retry-After` header (delta-seconds or an HTTP date), or null. */
export function retryAfterSeconds(header: string | null, now = Date.now()): number | null {
  if (!header) return null;
  const n = Number(header.trim());
  if (Number.isFinite(n)) return Math.max(0, Math.ceil(n));
  const at = Date.parse(header);
  return Number.isNaN(at) ? null : Math.max(0, Math.ceil((at - now) / 1000));
}

async function call<T>(
  method: "GET" | "POST" | "PUT" | "DELETE",
  path: string,
  body?: unknown,
): Promise<ApiResult<T>> {
  let res: Response;
  try {
    res = await fetch(base() + "/v1" + path, {
      method,
      credentials: "include",
      headers:
        body === undefined
          ? { accept: "application/json" }
          : { "content-type": "application/json", accept: "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch {
    return internal(0, true);
  }
  let parsed: unknown = null;
  if (res.status !== 204) {
    try {
      parsed = await res.json();
    } catch {
      parsed = null;
    }
  }
  if (res.status === 401) clearWizard();
  if (res.ok) return { ok: true, status: res.status, data: parsed as T };
  if (!isAppError(parsed)) return internal(res.status, false);
  const error: AppError = CODES.has(parsed.code) ? parsed : { ...parsed, code: "INTERNAL" };
  return {
    ok: false,
    status: res.status,
    error,
    retryAfterS: retryAfterSeconds(res.headers.get("retry-after")),
    offline: false,
  };
}

export const startSignup = (form: SignupStart) =>
  call<SignupChallenge>("POST", "/signup/start", form);
export const resendCode = (challenge_id: string) =>
  call<SignupChallenge>("POST", "/signup/resend", { challenge_id });
export const verifyCode = (challenge_id: string, code: string) =>
  signedInAgain(call<SignupVerified>("POST", "/signup/verify", { challenge_id, code }));
async function signedInAgain<T>(request: Promise<ApiResult<T>>): Promise<ApiResult<T>> {
  const res = await request;
  if (res.ok) clearWizard();
  return res;
}

export const getSession = () => call<SessionInfo>("GET", "/session");
export const refreshSession = () => call<SessionInfo>("POST", "/session/refresh");

/**
 * `GET /v1/session`; on a 401 (the 15-minute access cookie ran out) it rotates the session
 * once with `POST /v1/session/refresh` and asks again.
 */
export async function currentSession(): Promise<ApiResult<SessionInfo>> {
  const first = await getSession();
  if (first.ok || first.status !== 401) return first;
  const refreshed = await refreshSession();
  if (!refreshed.ok) return refreshed;
  return getSession();
}

async function withRefresh<T>(request: () => Promise<ApiResult<T>>): Promise<ApiResult<T>> {
  const first = await request();
  if (first.ok || first.error.code !== "AUTH_FAILED") return first;
  const refreshed = await refreshSession();
  if (refreshed.ok) return request();
  if (!refreshed.offline) await goto("/", { replaceState: true });
  return refreshed.offline ? refreshed : first;
}

export const setPassword = (password: string) =>
  withRefresh(() => call<null>("POST", "/account/password", { password }));
export const startTotp = () =>
  withRefresh(() => call<TotpStarted>("POST", "/account/mfa/totp/start"));
export const confirmTotp = (code: string) =>
  withRefresh(() => call<BackupCodes>("POST", "/account/mfa/totp/confirm", { code }));
export const ackBackupCodes = () =>
  withRefresh(() => call<null>("POST", "/account/mfa/backup-codes/ack"));
export const regenerateBackupCodes = (code: string) =>
  withRefresh(() => call<BackupCodes>("POST", "/account/mfa/backup-codes/regenerate", { code }));

export const signIn = (login: string, password: string) =>
  signedInAgain(call<SigninReply>("POST", "/signin", { login, password }));
export const sendSigninSms = (challenge_id: string) =>
  call<SigninSmsSent>("POST", "/signin/sms", { challenge_id });
export const verifySignin = (
  challenge_id: string,
  method: SigninMethod,
  code: string,
  trust_device: boolean,
) =>
  signedInAgain(
    call<SigninVerified>("POST", "/signin/verify", { challenge_id, method, code, trust_device }),
  );
export const setSmsFallback = (enabled: boolean) =>
  withRefresh(() => call<null>("PUT", "/account/security/sms-fallback", { enabled }));
export const listDevices = () =>
  withRefresh(() => call<TrustedDevice[]>("GET", "/account/devices"));
export const removeDevice = (id: string) =>
  withRefresh(() => call<null>("DELETE", `/account/devices/${encodeURIComponent(id)}`));
export const signOutEverywhere = async () => {
  const res = await withRefresh(() => call<null>("POST", "/account/sign-out-everywhere"));
  if (res.ok) clearWizard();
  return res;
};

export const eventsUrl = () => base() + "/v1" + "/wizard/events";
export const getWizard = () => withRefresh(() => call<WizardState>("GET", "/wizard"));
export const saveCafe = (details: CafeDetails) =>
  withRefresh(() => call<WizardState>("PUT", "/wizard/cafe", details));
export const setBranchCode = (branchId: string, code: string) =>
  withRefresh(() =>
    call<BranchCodeSet>("PUT", `/branches/${encodeURIComponent(branchId)}/code`, { code }),
  );
export const listCountries = () => call<Country[]>("GET", "/meta/countries");
export const listTimezones = () => call<string[]>("GET", "/meta/timezones");
export const getTierChoices = () => withRefresh(() => call<TierChoices>("GET", "/wizard/tier"));
export const chooseTier = (tier: Tier, branchId?: string) =>
  withRefresh(() =>
    call<WizardState>("PUT", "/wizard/tier", branchId ? { tier, branch_id: branchId } : { tier }),
  );
export const getInstallHub = () =>
  withRefresh(() => call<InstallHubState>("GET", "/wizard/install-hub"));
export const revealSetupCode = () =>
  withRefresh(() => call<SetupCodeReveal>("POST", "/wizard/setup-code/reveal", {}));
export const issuePairingCode = () =>
  withRefresh(() => call<PairingCodeIssued>("POST", "/wizard/install-hub/code", {}));
export const getStaff = () => withRefresh(() => call<StaffInvites>("GET", "/wizard/staff"));
export const inviteStaff = (invite: StaffInvite) =>
  withRefresh(() => call<StaffInvited>("POST", "/wizard/staff", invite));
export const newStaffCode = (id: string) =>
  withRefresh(() => call<StaffInvited>("POST", `/wizard/staff/${encodeURIComponent(id)}/code`, {}));
export const skipStaff = () =>
  withRefresh(() => call<WizardState>("POST", "/wizard/staff/skip", {}));
