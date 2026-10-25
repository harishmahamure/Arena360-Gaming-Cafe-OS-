#!/usr/bin/env node
// Deploy check: renders both arena-cloud overlays with `kubectl kustomize` (local files only)
// and asserts the deployment and backup CronJob invariants, then checks the cluster config, the image and the
// deploy workflow statically. Nothing is applied or contacted.

import { execFileSync } from "node:child_process";
import { readFileSync, realpathSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { isDeepStrictEqual } from "node:util";
import { parse as parseYaml, parseAllDocuments } from "yaml";

export const ENVIRONMENTS = {
  staging: {
    namespace: "staging",
    host: "api.staging.arena360.cloud",
    bucket: "arena360-gz-control-backup-staging",
    webOrigins: "https://start.staging.arena360.cloud",
  },
  production: {
    namespace: "production",
    host: "api.arena360.cloud",
    bucket: "arena360-gz-control-backup-production",
    webOrigins: "https://start.arena360.cloud",
  },
};
export const DEPLOYMENT = "arena-cloud-control";
export const SECRET = "arena-cloud-secrets";
export const SECRET_KEYS = ["ARENA_CONTROL_DB_TOKEN", "ARENA_CONTROL_DB_URL"];
/** The Deployment's secret env (Story 2.4 adds the OTP pepper and the Gupshup account, Story
 * 2.5 the MFA key), all from `arena-cloud-secrets`. */
export const DEPLOYMENT_SECRET_KEYS = [
  "ARENA_CONTROL_DB_TOKEN",
  "ARENA_CONTROL_DB_URL",
  "ARENA_GUPSHUP_DLT_TEMPLATE_ID",
  "ARENA_GUPSHUP_ENDPOINT",
  "ARENA_GUPSHUP_PASSWORD",
  "ARENA_GUPSHUP_SENDER",
  "ARENA_GUPSHUP_USERID",
  "ARENA_LICENCE_KID",
  "ARENA_LICENCE_SIGNING_KEY",
  "ARENA_MFA_KEY",
  "ARENA_OTP_PEPPER",
];
/** The Deployment's only plain env: the SMS provider and the per-environment web origins. */
export const SMS_PROVIDER = "gupshup";
export const MIDDLEWARE = "cloudflare-only";
/** The nightly gz_control backup (Story 2.3). */
export const CRONJOB = "arena-cloud-control-backup";
export const BACKUP_SECRET = "arena-cloud-backup";
/** The CronJob's secret env: secret name -> keys (each env var reads the key of its name). */
export const CRONJOB_SECRET_KEYS = {
  [SECRET]: SECRET_KEYS,
  [BACKUP_SECRET]: [
    "ARENA_ALERT_WEBHOOK_URL",
    "ARENA_BACKUP_AGE_RECIPIENTS",
    "ARENA_BACKUP_HEARTBEAT_URL",
    "ARENA_BACKUP_S3_ACCESS_KEY_ID",
    "ARENA_BACKUP_S3_ENDPOINT",
    "ARENA_BACKUP_S3_SECRET_ACCESS_KEY",
  ],
};
/** The only CronJob secret keys that may be `optional: true` (both are optional in the binary). */
export const CRONJOB_OPTIONAL_KEYS = ["ARENA_ALERT_WEBHOOK_URL", "ARENA_BACKUP_HEARTBEAT_URL"];
export const CRONJOB_ARGS = ["--role", "jobs", "--task", "control-backup"];
export const CRONJOB_SCHEDULE = "30 2 * * *";
export const CRONJOB_TIME_ZONE = "Asia/Kolkata";
/** Every Secret a deploy applies (the deployer Role may update only these). */
export const DEPLOYED_SECRETS = [SECRET, BACKUP_SECRET, "origin-tls", "ghcr-pull"];
export const SOPS_VERSION = "3.13.3";
export const AGE_VERSION = "1.3.2";
const MW_ANNOTATION = "traefik.ingress.kubernetes.io/router.middlewares";
const CLUSTER_SCOPED = new Set([
  "Namespace",
  "HelmChartConfig",
  "ClusterRole",
  "ClusterRoleBinding",
]);
/** What a deploy applies in a namespace, as `apiGroup/resource` (the deployer Role's scope). */
export const DEPLOYER_RESOURCES = [
  "apps/deployments",
  "/services",
  "networking.k8s.io/ingresses",
  "traefik.io/middlewares",
  "batch/cronjobs",
  "/secrets",
];
export const KUBECTL_MISSING = "check:deploy needs kubectl on PATH";

/** Parses multi-document YAML into plain objects (empty documents dropped). */
export function parseDocs(text) {
  return parseAllDocuments(text)
    .map((d) => {
      if (d.errors.length > 0) throw new Error(d.errors[0].message);
      return d.toJS();
    })
    .filter((d) => d && typeof d === "object");
}

/** One CIDR per line; blank lines and `#` comments ignored. */
export function readRanges(text) {
  return text
    .split(/\r?\n/)
    .map((l) => l.replace(/#.*/, "").trim())
    .filter(Boolean);
}

const find = (docs, kind, name) =>
  docs.find((d) => d.kind === kind && (name === undefined || d.metadata?.name === name));

/**
 * Checks one rendered overlay.
 * @param {string} env environment name (for messages)
 * @param {object[]} docs rendered documents
 * @param {{namespace: string, host: string}} want
 * @param {string[]} ranges the committed Cloudflare ranges (v4 + v6)
 * @returns {string[]} error messages
 */
export function checkOverlay(env, docs, want, ranges) {
  const errors = [];
  const err = (m) => errors.push(`${env}: ${m}`);
  const ns = want.namespace;

  for (const d of docs) {
    // The namespace-scoped deployer cannot create cluster-scoped objects; namespaces come
    // from deploy/k8s/cluster/deployer-rbac.yaml (an operator step).
    if (CLUSTER_SCOPED.has(d.kind)) err(`renders cluster-scoped ${d.kind} ${d.metadata?.name}`);
    if (d.kind === "Secret") err(`renders a Secret ${d.metadata?.name ?? ""}`.trim());
    if (!CLUSTER_SCOPED.has(d.kind) && d.metadata?.namespace !== ns) {
      err(`${d.kind} ${d.metadata?.name} is in namespace ${d.metadata?.namespace}, not ${ns}`);
    }
  }

  const dep = find(docs, "Deployment", DEPLOYMENT);
  if (!dep) {
    err(`no Deployment ${DEPLOYMENT}`);
  } else {
    const pod = dep.spec?.template?.spec ?? {};
    const containers = pod.containers ?? [];
    if (containers.length !== 1) err(`${DEPLOYMENT} must have one container`);
    for (const c of containers) {
      if (!isDeepStrictEqual(c.args, ["--role", "control"])) {
        err(`container ${c.name} args are ${JSON.stringify(c.args)}, not ["--role","control"]`);
      }
      if (c.envFrom) err(`container ${c.name} uses envFrom`);
      const keys = [];
      const plain = { ARENA_SMS_PROVIDER: SMS_PROVIDER, ARENA_WEB_ORIGINS: want.webOrigins };
      const seenPlain = new Set();
      for (const e of c.env ?? []) {
        if (e.name in plain) {
          if (e.value !== plain[e.name]) {
            err(
              `env ${e.name} is ${JSON.stringify(e.value)}, not ${JSON.stringify(plain[e.name])}`,
            );
          }
          seenPlain.add(e.name);
          continue;
        }
        const ref = e.valueFrom?.secretKeyRef;
        if (!ref || "value" in e) {
          err(`env ${e.name} is not a secretKeyRef`);
          continue;
        }
        if (ref.name !== SECRET) err(`env ${e.name} reads secret ${ref.name}, not ${SECRET}`);
        if (ref.key !== e.name) err(`env ${e.name} reads key ${ref.key}`);
        if (ref.optional) err(`env ${e.name} is optional`);
        keys.push(ref.key);
      }
      for (const name of Object.keys(plain)) {
        if (!seenPlain.has(name)) err(`env ${name} is missing`);
      }
      if (!isDeepStrictEqual([...keys].sort(), DEPLOYMENT_SECRET_KEYS)) {
        err(`env keys are ${JSON.stringify(keys)}, want ${JSON.stringify(DEPLOYMENT_SECRET_KEYS)}`);
      }
      for (const p of ["livenessProbe", "readinessProbe"]) {
        if (c[p]?.httpGet?.path !== "/healthz") err(`${p} is not GET /healthz`);
      }
      const r = c.resources ?? {};
      for (const k of ["requests", "limits"]) {
        if (!r[k]?.cpu || !r[k]?.memory) err(`resources.${k} cpu and memory must be set`);
      }
      if (!(c.lifecycle?.preStop?.sleep?.seconds > 0)) {
        err("container has no lifecycle.preStop.sleep (rollouts would drop requests)");
      }
      const sc = c.securityContext ?? {};
      if (sc.readOnlyRootFilesystem !== true) err("readOnlyRootFilesystem is not true");
      if (sc.allowPrivilegeEscalation !== false) err("allowPrivilegeEscalation is not false");
      if (sc.runAsNonRoot !== true && pod.securityContext?.runAsNonRoot !== true) {
        err("runAsNonRoot is not true");
      }
    }
    if (pod.securityContext?.runAsNonRoot !== true) err("pod runAsNonRoot is not true");
  }

  const ingresses = docs.filter((d) => d.kind === "Ingress");
  if (ingresses.length === 0) err("no Ingress");
  for (const ing of ingresses) {
    const name = ing.metadata?.name;
    const mws = String(ing.metadata?.annotations?.[MW_ANNOTATION] ?? "")
      .split(",")
      .map((s) => s.trim());
    if (!mws.includes(`${ns}-${MIDDLEWARE}@kubernetescrd`)) {
      err(`Ingress ${name} routers lack the ${ns}-${MIDDLEWARE}@kubernetescrd middleware`);
    }
    const hosts = (ing.spec?.rules ?? []).map((r) => r.host);
    if (!isDeepStrictEqual(hosts, [want.host])) {
      err(`Ingress ${name} hosts are ${JSON.stringify(hosts)}, not ["${want.host}"]`);
    }
    if (ing.spec?.defaultBackend) err(`Ingress ${name} has a defaultBackend (a hostless router)`);
    const tls = ing.spec?.tls ?? [];
    if (
      tls.length !== 1 ||
      tls[0].secretName !== "origin-tls" ||
      !isDeepStrictEqual(tls[0].hosts, [want.host])
    ) {
      err(`Ingress ${name} TLS must be origin-tls for ${want.host}`);
    }
  }
  for (const route of docs.filter((d) => d.kind === "IngressRoute")) {
    for (const r of route.spec?.routes ?? []) {
      if (!(r.middlewares ?? []).some((m) => m.name === MIDDLEWARE)) {
        err(`IngressRoute ${route.metadata?.name} has a route without ${MIDDLEWARE}`);
      }
    }
  }

  const mw = find(docs, "Middleware", MIDDLEWARE);
  if (!mw) {
    err(`no Middleware ${MIDDLEWARE}`);
  } else {
    const got = mw.spec?.ipAllowList?.sourceRange ?? [];
    const a = [...got].sort();
    const b = [...ranges].sort();
    if (!isDeepStrictEqual(a, b)) {
      const missing = b.filter((x) => !a.includes(x));
      const extra = a.filter((x) => !b.includes(x));
      err(
        `Middleware ${MIDDLEWARE} allow list differs from deploy/cloudflare (missing ${JSON.stringify(missing)}, extra ${JSON.stringify(extra)}${a.length !== new Set(a).size ? ", duplicates" : ""})`,
      );
    }
  }
  return errors;
}

/**
 * The nightly backup CronJob in one rendered overlay: schedule, time zone, Forbid, deadlines,
 * history, the Deployment's image and security context, and its env (secret refs only, plus
 * the namespace as ARENA_ENV and this environment's own bucket).
 * @returns {string[]} error messages
 */
export function checkCronJob(env, docs, want) {
  const errors = [];
  const err = (m) => errors.push(`${env}: CronJob ${CRONJOB}: ${m}`);
  const cj = find(docs, "CronJob", CRONJOB);
  if (!cj) return [`${env}: no CronJob ${CRONJOB}`];
  const spec = cj.spec ?? {};
  const expect = (path, got, wantValue) => {
    if (!isDeepStrictEqual(got, wantValue)) {
      err(`${path} is ${JSON.stringify(got)}, not ${JSON.stringify(wantValue)}`);
    }
  };
  expect("schedule", spec.schedule, CRONJOB_SCHEDULE);
  expect("timeZone", spec.timeZone, CRONJOB_TIME_ZONE);
  expect("concurrencyPolicy", spec.concurrencyPolicy, "Forbid");
  expect("startingDeadlineSeconds", spec.startingDeadlineSeconds, 3600);
  expect("successfulJobsHistoryLimit", spec.successfulJobsHistoryLimit, 3);
  expect("failedJobsHistoryLimit", spec.failedJobsHistoryLimit, 5);
  const job = spec.jobTemplate?.spec ?? {};
  expect("backoffLimit", job.backoffLimit, 0);
  expect("activeDeadlineSeconds", job.activeDeadlineSeconds, 1800);
  const pod = job.template?.spec ?? {};
  expect("restartPolicy", pod.restartPolicy, "Never");
  if (pod.automountServiceAccountToken !== false) err("automountServiceAccountToken is not false");
  const containers = pod.containers ?? [];
  if (containers.length !== 1) err("must have one container");

  const dep = find(docs, "Deployment", DEPLOYMENT);
  const depPod = dep?.spec?.template?.spec ?? {};
  const depContainer = depPod.containers?.[0] ?? {};
  if (!isDeepStrictEqual(pod.securityContext, depPod.securityContext)) {
    err("pod securityContext differs from the Deployment's");
  }
  if (!isDeepStrictEqual(pod.imagePullSecrets, depPod.imagePullSecrets)) {
    err("imagePullSecrets differ from the Deployment's");
  }
  for (const c of containers) {
    expect("args", c.args, CRONJOB_ARGS);
    if (c.image !== depContainer.image) {
      err(`image ${c.image} is not the Deployment's ${depContainer.image}`);
    }
    if (!isDeepStrictEqual(c.securityContext, depContainer.securityContext)) {
      err("container securityContext differs from the Deployment's");
    }
    if (c.envFrom) err("uses envFrom");
    const r = c.resources ?? {};
    for (const k of ["requests", "limits"]) {
      if (!r[k]?.cpu || !r[k]?.memory) err(`resources.${k} cpu and memory must be set`);
    }
    const got = {};
    const plain = {};
    for (const e of c.env ?? []) {
      const ref = e.valueFrom?.secretKeyRef;
      if (e.name === "ARENA_ENV") {
        if (e.valueFrom?.fieldRef?.fieldPath !== "metadata.namespace" || "value" in e) {
          err("ARENA_ENV is not the pod's namespace (fieldRef metadata.namespace)");
        }
        plain[e.name] = true;
        continue;
      }
      if (e.name === "ARENA_BACKUP_S3_BUCKET") {
        if (e.value !== want.bucket) {
          err(`ARENA_BACKUP_S3_BUCKET is ${JSON.stringify(e.value)}, not ${want.bucket}`);
        }
        plain[e.name] = true;
        continue;
      }
      if (!ref || "value" in e) {
        err(`env ${e.name} is not a secretKeyRef`);
        continue;
      }
      if (!(ref.name in CRONJOB_SECRET_KEYS)) err(`env ${e.name} reads secret ${ref.name}`);
      if (ref.key !== e.name) err(`env ${e.name} reads key ${ref.key}`);
      if (ref.optional && !CRONJOB_OPTIONAL_KEYS.includes(ref.key)) {
        err(`env ${e.name} is optional`);
      }
      (got[ref.name] ??= []).push(ref.key);
    }
    for (const name of ["ARENA_ENV", "ARENA_BACKUP_S3_BUCKET"]) {
      if (!plain[name]) err(`no ${name}`);
    }
    for (const [secret, keys] of Object.entries(CRONJOB_SECRET_KEYS)) {
      const have = [...(got[secret] ?? [])].sort();
      if (!isDeepStrictEqual(have, keys)) {
        err(`keys from ${secret} are ${JSON.stringify(have)}, want ${JSON.stringify(keys)}`);
      }
    }
  }
  return errors;
}

/** Blanks the fields the environments may differ in: namespace, host, replicas, resources,
 * image tag. */
export function normalise(docs, want) {
  const text = JSON.stringify(docs)
    .replaceAll(want.host, "<host>")
    .replaceAll(
      `"${want.namespace}-${MIDDLEWARE}@kubernetescrd"`,
      `"<ns>-${MIDDLEWARE}@kubernetescrd"`,
    );
  const out = JSON.parse(text);
  for (const d of out) {
    if (d.metadata?.namespace === want.namespace) d.metadata.namespace = "<ns>";
    if (d.kind === "Namespace" && d.metadata?.name === want.namespace) d.metadata.name = "<ns>";
    if (d.kind === "Deployment") {
      if (d.spec) d.spec.replicas = "<replicas>";
      for (const c of d.spec?.template?.spec?.containers ?? []) {
        c.resources = "<resources>";
        for (const e of c.env ?? []) {
          if (e.name === "ARENA_WEB_ORIGINS" && e.value === want.webOrigins) e.value = "<origins>";
        }
        if (typeof c.image === "string") c.image = c.image.replace(/:[^:/@]+$/, ":<tag>");
      }
    }
    if (d.kind === "CronJob") {
      for (const c of d.spec?.jobTemplate?.spec?.template?.spec?.containers ?? []) {
        if (typeof c.image === "string") c.image = c.image.replace(/:[^:/@]+$/, ":<tag>");
        for (const e of c.env ?? []) {
          if (e.name === "ARENA_BACKUP_S3_BUCKET" && e.value === want.bucket) e.value = "<bucket>";
        }
      }
    }
  }
  return out;
}

/** Staging and production may differ only in namespace, host, replicas/resources, tag,
 * backup bucket and web origins. */
export function checkParity(stagingDocs, productionDocs) {
  const a = normalise(stagingDocs, ENVIRONMENTS.staging);
  const b = normalise(productionDocs, ENVIRONMENTS.production);
  if (isDeepStrictEqual(a, b)) return [];
  return [
    "staging and production differ beyond namespace, host, replicas/resources, image tag, backup bucket and web origins",
  ];
}

/** deploy/k8s/cluster/traefik-config.yaml: Traefik keeps the real source IP. */
/** deploy/k8s/cluster/deployer-rbac.yaml: per environment a Namespace, ServiceAccount
 * `deployer`, a Role limited to what a deploy applies there, and its RoleBinding. */
export function checkRbac(text) {
  const errors = [];
  const docs = parseDocs(text);
  for (const d of docs) {
    if (d.kind === "ClusterRole" || d.kind === "ClusterRoleBinding") {
      errors.push(`rbac: ${d.kind} ${d.metadata?.name} grants cluster-wide access`);
    }
  }
  for (const { namespace: ns } of Object.values(ENVIRONMENTS)) {
    const inNs = (kind) =>
      docs.find(
        (d) => d.kind === kind && d.metadata?.name === "deployer" && d.metadata?.namespace === ns,
      );
    if (!docs.some((d) => d.kind === "Namespace" && d.metadata?.name === ns)) {
      errors.push(`rbac: no Namespace ${ns}`);
    }
    if (!inNs("ServiceAccount")) errors.push(`rbac: no ServiceAccount deployer in ${ns}`);
    const role = inNs("Role");
    if (!role) {
      errors.push(`rbac: no Role deployer in ${ns}`);
    } else {
      const rules = role.rules ?? [];
      for (const r of rules) {
        if ([...(r.verbs ?? []), ...(r.resources ?? []), ...(r.apiGroups ?? [])].includes("*")) {
          errors.push(`rbac: Role deployer in ${ns} has a wildcard rule`);
        }
        if (
          (r.verbs ?? []).some(
            (v) =>
              v === "delete" ||
              v === "deletecollection" ||
              v === "escalate" ||
              v === "bind" ||
              v === "impersonate",
          )
        ) {
          errors.push(`rbac: Role deployer in ${ns} grants ${r.verbs.join(",")}`);
        }
      }
      const secretNames = rules
        .filter((r) => (r.resources ?? []).includes("secrets") && r.resourceNames)
        .flatMap((r) => r.resourceNames);
      for (const name of DEPLOYED_SECRETS) {
        if (!secretNames.includes(name)) {
          errors.push(`rbac: Role deployer in ${ns} cannot update Secret ${name}`);
        }
      }
      for (const want of DEPLOYER_RESOURCES) {
        const [group, resource] = want.split("/");
        const covered = rules.some(
          (r) =>
            (r.apiGroups ?? []).includes(group) &&
            (r.resources ?? []).includes(resource) &&
            (r.verbs ?? []).includes(resource === "secrets" ? "create" : "patch"),
        );
        if (!covered) errors.push(`rbac: Role deployer in ${ns} cannot apply ${want}`);
      }
    }
    const rb = inNs("RoleBinding");
    if (
      !rb ||
      rb.roleRef?.kind !== "Role" ||
      rb.roleRef?.name !== "deployer" ||
      !(rb.subjects ?? []).some(
        (s) => s.kind === "ServiceAccount" && s.name === "deployer" && s.namespace === ns,
      )
    ) {
      errors.push(`rbac: no RoleBinding of Role deployer to ServiceAccount deployer in ${ns}`);
    }
  }
  return errors;
}

export function checkCluster(text) {
  const docs = parseDocs(text);
  const hcc = docs.find((d) => d.kind === "HelmChartConfig" && d.metadata?.name === "traefik");
  if (!hcc || hcc.metadata?.namespace !== "kube-system") {
    return ["cluster: no HelmChartConfig traefik in kube-system"];
  }
  const values = parseYaml(hcc.spec?.valuesContent ?? "") ?? {};
  if (values.service?.spec?.externalTrafficPolicy !== "Local") {
    return ["cluster: Traefik service externalTrafficPolicy is not Local"];
  }
  return [];
}

/** The Dockerfile: toolchain from rust-toolchain.toml, locked release build, distroless/cc
 * pinned by tag, non-root. */
export function checkDockerfile(text, rustChannel) {
  const errors = [];
  const froms = [...text.matchAll(/^FROM\s+(\S+)(?:\s+AS\s+\S+)?\s*$/gim)].map((m) => m[1]);
  if (froms.length < 2) errors.push("Dockerfile: expected a multi-stage build");
  if (!froms[0]?.startsWith(`rust:${rustChannel}`)) {
    errors.push(
      `Dockerfile: build stage ${froms[0]} is not rust:${rustChannel} (rust-toolchain.toml)`,
    );
  }
  const runtime = froms.at(-1) ?? "";
  const m = runtime.match(/^gcr\.io\/distroless\/cc[^:@]*:([^@\s]+)/);
  if (!m || m[1] === "latest") {
    errors.push(`Dockerfile: runtime ${runtime} is not distroless/cc pinned by tag`);
  }
  if (!/cargo build --locked --release -p arena-cloud/.test(text)) {
    errors.push("Dockerfile: missing cargo build --locked --release -p arena-cloud");
  }
  const user = [...text.matchAll(/^USER\s+(\S+)/gim)].at(-1)?.[1];
  if (!user || /^(0|root)(:|$)/.test(user)) errors.push("Dockerfile: runtime user is not non-root");
  return errors;
}

/** The deploy workflow, statically. Returns {errors, warnings}. */
export function checkWorkflow(text) {
  const errors = [];
  const warnings = [];
  const wf = parseYaml(text);
  const push = wf?.on?.push ?? {};
  if (!isDeepStrictEqual(push.branches, ["master"]))
    errors.push("workflow: push.branches must be [master]");
  if (!isDeepStrictEqual(push.tags, ["v*"])) errors.push("workflow: push.tags must be [v*]");
  const env = wf?.env ?? {};
  if (env.SOPS_VERSION !== SOPS_VERSION)
    errors.push(`workflow: SOPS_VERSION is not ${SOPS_VERSION}`);
  if (env.AGE_VERSION !== AGE_VERSION) errors.push(`workflow: AGE_VERSION is not ${AGE_VERSION}`);
  if (!String(env.SOPS_URL ?? "").includes(`/v${SOPS_VERSION}/sops-v${SOPS_VERSION}.`)) {
    errors.push("workflow: SOPS_URL is not the pinned release");
  }
  if (!String(env.AGE_URL ?? "").includes(`/v${AGE_VERSION}/age-v${AGE_VERSION}-`)) {
    errors.push("workflow: AGE_URL is not the pinned release");
  }
  for (const k of ["SOPS_SHA256", "AGE_SHA256"]) {
    const v = String(env[k] ?? "");
    if (v === "PIN-ME") warnings.push(`workflow: ${k} is not pinned yet (operator step)`);
    else if (!/^[0-9a-f]{64}$/.test(v)) errors.push(`workflow: ${k} is not a sha256`);
  }
  const jobs = wf?.jobs ?? {};
  const ciJob = Object.entries(jobs).find(([, j]) => j.uses === "./.github/workflows/ci.yml");
  if (!ciJob) errors.push("workflow: no job calls ./.github/workflows/ci.yml");
  const build = jobs.build;
  const needs = (j) => [j?.needs ?? []].flat();
  const notTag = /^\$\{\{\s*!startsWith\(github\.ref, 'refs\/tags\/v'\)\s*\}\}$/;
  const onTag = /^\$\{\{\s*startsWith\(github\.ref, 'refs\/tags\/v'\)\s*\}\}$/;
  if (!build) {
    errors.push("workflow: no build job");
  } else {
    if (ciJob && !needs(build).includes(ciJob[0])) errors.push("workflow: build does not need ci");
    const steps = build.steps ?? [];
    const runOf = (re) => steps.filter((s) => re.test(s.run ?? ""));
    const builds = runOf(/docker (build|push)/);
    if (builds.length < 2) errors.push("workflow: build job does not build and push the image");
    if (builds.some((s) => !notTag.test(String(s.if ?? "")))) {
      errors.push("workflow: v* tags must not rebuild or push the image");
    }
    if (!steps.some((s) => /\$\{\{\s*github\.sha\s*\}\}/.test(s.run ?? ""))) {
      errors.push("workflow: the image is not tagged with the commit SHA");
    }
    if (
      !runOf(/docker manifest inspect[\s\S]*exit 1/).some((s) => onTag.test(String(s.if ?? "")))
    ) {
      errors.push("workflow: a v* tag does not verify the staged image exists");
    }
    if (
      !runOf(/@sha256:/).length ||
      !/steps\.digest\.outputs\.image/.test(String(build.outputs?.image ?? ""))
    ) {
      errors.push("workflow: the deployed image is not resolved to its digest");
    }
  }
  const deploy = jobs.deploy;
  if (!deploy) {
    errors.push("workflow: no deploy job");
    return { errors, warnings };
  }
  if (!needs(deploy).includes("build")) errors.push("workflow: deploy does not need build");
  if (!/needs\.build\.outputs\.image/.test(String(deploy.env?.IMAGE ?? ""))) {
    errors.push("workflow: deploy IMAGE is not the build job's digest");
  }
  const envExpr = "startsWith(github.ref, 'refs/tags/v') && 'production' || 'staging'";
  if (!String(deploy.environment ?? "").includes(envExpr)) {
    errors.push("workflow: deploy environment must be production on v* tags and staging on master");
  }
  const runs = (deploy.steps ?? []).map((s) => `${s.name ?? ""}\n${s.run ?? ""}`);
  const stepEnv = (deploy.steps ?? []).map((s) => s.env ?? {});
  const has = (re) => runs.some((r) => re.test(r));
  if (!has(/sha256sum -c[\s\S]*sha256sum -c/))
    errors.push("workflow: sops and age are not sha256-verified");
  if (!stepEnv.some((e) => /secrets\.SOPS_AGE_KEY/.test(String(e.SOPS_AGE_KEY ?? "")))) {
    errors.push("workflow: SOPS_AGE_KEY does not come from the environment secret");
  }
  if (!has(/deploy\/secrets\/\$\{DEPLOY_ENV\}\.enc\.yaml[\s\S]*missing/)) {
    errors.push("workflow: no clear failure when the encrypted secrets file is missing");
  }
  if (!has(/sops --decrypt[\s\S]*\|\s*ssh /))
    errors.push("workflow: secrets are not decrypted into ssh kubectl apply");
  if (
    !stepEnv.some((e) =>
      /secrets\.DEPLOY_SSH_KNOWN_HOSTS/.test(String(e.DEPLOY_SSH_KNOWN_HOSTS ?? "")),
    )
  ) {
    errors.push("workflow: known_hosts is not pinned from a secret");
  }
  if (!has(/StrictHostKeyChecking yes/)) errors.push("workflow: StrictHostKeyChecking is not on");
  if (!has(/kubectl kustomize "deploy\/k8s\/overlays\/\$\{DEPLOY_ENV\}"/)) {
    errors.push("workflow: the overlay is not rendered");
  }
  if (
    !has(
      /select\(\.kind == "CronJob" and \.metadata\.name == "arena-cloud-control-backup"\)[^\n]*strenv\(IMAGE\)/,
    )
  ) {
    errors.push("workflow: the backup CronJob's image is not set to the deployed image");
  }
  if (!has(/rollout status deployment\/arena-cloud-control/))
    errors.push("workflow: no rollout status wait");
  if (!has(/\/healthz[\s\S]*migration_version[\s\S]*reachable == true/)) {
    errors.push("workflow: no edge /healthz smoke (migration_version, reachable)");
  }
  if (!has(/http:\/\/\$\{EDGE_HOST\}[\s\S]*301[\s\S]*308/))
    errors.push("workflow: no http -> https smoke");
  if (!has(/--resolve[\s\S]*403/)) errors.push("workflow: no direct-origin smoke");
  const code = text.replace(/^\s*#.*$/gm, "");
  if (/restore-control/.test(code)) {
    errors.push("workflow: a deploy must never run ops restore-control");
  }
  if (/kube-?apiserver|:6443/.test(code)) {
    errors.push("workflow: talks to a kube-apiserver directly");
  }
  if (/deploy\/k8s\/cluster|create namespace|kind: Namespace/.test(code)) {
    errors.push("workflow: applies cluster-wide config or namespaces (one-time operator steps)");
  }
  return { errors, warnings };
}

/** ci.yml must be callable, so the deploy can run it first. */
export function checkCiWorkflow(text) {
  const on = parseYaml(text)?.on ?? {};
  return typeof on === "object" && "workflow_call" in on
    ? []
    : ["ci.yml: no workflow_call trigger (deploy-cloud.yml runs CI first)"];
}

export const PAIR_RATE_LIMIT = { path: "/v1/pair", period: 3600, requests: 20 };

export function checkRateLimits(text) {
  let config;
  try {
    config = JSON.parse(text);
  } catch (e) {
    return [`deploy/cloudflare/rate-limits.json: not JSON (${e.message})`];
  }
  const rules = Array.isArray(config?.rules) ? config.rules : [];
  const rule = rules.find((r) => String(r?.expression ?? "").includes(`"${PAIR_RATE_LIMIT.path}"`));
  if (!rule) return [`deploy/cloudflare/rate-limits.json: no rule for ${PAIR_RATE_LIMIT.path}`];
  const errors = [];
  const rl = rule.ratelimit ?? {};
  if (rl.requests_per_period !== PAIR_RATE_LIMIT.requests || rl.period !== PAIR_RATE_LIMIT.period) {
    errors.push(
      `deploy/cloudflare/rate-limits.json: ${PAIR_RATE_LIMIT.path} must allow ${PAIR_RATE_LIMIT.requests} per ${PAIR_RATE_LIMIT.period} s (found ${rl.requests_per_period} per ${rl.period} s)`,
    );
  }
  if (!Array.isArray(rl.characteristics) || rl.characteristics.join() !== "ip.src") {
    errors.push(
      `deploy/cloudflare/rate-limits.json: ${PAIR_RATE_LIMIT.path} must count per ip.src`,
    );
  }
  if (rule.action !== "block") {
    errors.push(
      `deploy/cloudflare/rate-limits.json: ${PAIR_RATE_LIMIT.path} must block over the limit`,
    );
  }
  return errors;
}

export function render(root, env) {
  try {
    return execFileSync("kubectl", ["kustomize", join(root, "deploy/k8s/overlays", env)], {
      encoding: "utf8",
      maxBuffer: 16 * 1024 * 1024,
    });
  } catch (e) {
    if (e.code === "ENOENT") throw new Error(KUBECTL_MISSING);
    throw e;
  }
}

export function checkRepo(root, renderOverlay = (env) => render(root, env)) {
  const read = (p) => readFileSync(join(root, p), "utf8");
  const ranges = [
    ...readRanges(read("deploy/cloudflare/ips-v4.txt")),
    ...readRanges(read("deploy/cloudflare/ips-v6.txt")),
  ];
  const errors = [];
  const rendered = {};
  for (const [env, want] of Object.entries(ENVIRONMENTS)) {
    rendered[env] = parseDocs(renderOverlay(env));
    errors.push(...checkOverlay(env, rendered[env], want, ranges));
    errors.push(...checkCronJob(env, rendered[env], want));
  }
  errors.push(...checkParity(rendered.staging, rendered.production));
  errors.push(...checkCluster(read("deploy/k8s/cluster/traefik-config.yaml")));
  errors.push(...checkRateLimits(read("deploy/cloudflare/rate-limits.json")));
  errors.push(...checkRbac(read("deploy/k8s/cluster/deployer-rbac.yaml")));
  errors.push(...checkCiWorkflow(read(".github/workflows/ci.yml")));
  const channel = read("rust-toolchain.toml").match(/^\s*channel\s*=\s*"([^"]+)"/m)?.[1];
  errors.push(...checkDockerfile(read("deploy/docker/arena-cloud.Dockerfile"), channel));
  const wf = checkWorkflow(read(".github/workflows/deploy-cloud.yml"));
  errors.push(...wf.errors);
  return { errors, warnings: wf.warnings };
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  let result;
  try {
    result = checkRepo(root);
  } catch (e) {
    console.error(e.message === KUBECTL_MISSING ? KUBECTL_MISSING : `check:deploy: ${e.message}`);
    process.exit(1);
  }
  const { errors, warnings } = result;
  for (const w of warnings) console.warn(`warning: ${w}`);
  if (errors.length > 0) {
    for (const e of errors) console.error(e);
    process.exit(1);
  }
  console.log("deploy check: ok");
}

const isMain = (() => {
  try {
    return (
      Boolean(process.argv[1]) &&
      realpathSync(fileURLToPath(import.meta.url)) === realpathSync(process.argv[1])
    );
  } catch {
    return false;
  }
})();

if (isMain) {
  main();
}
