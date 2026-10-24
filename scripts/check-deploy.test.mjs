import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  ENVIRONMENTS,
  KUBECTL_MISSING,
  checkCiWorkflow,
  checkCluster,
  checkCronJob,
  checkDockerfile,
  checkRbac,
  checkOverlay,
  checkParity,
  checkRepo,
  checkWorkflow,
  parseDocs,
  readRanges,
  render,
} from "./check-deploy.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");
const RANGES = [
  ...readRanges(read("deploy/cloudflare/ips-v4.txt")),
  ...readRanges(read("deploy/cloudflare/ips-v6.txt")),
];
// Rendered once; without kubectl only the tests that need it fail (with that message).
let rendered;
let renderError;
try {
  rendered = {
    staging: parseDocs(render(root, "staging")),
    production: parseDocs(render(root, "production")),
  };
} catch (e) {
  renderError = e;
}
const copy = (env) => {
  if (renderError) throw renderError;
  return structuredClone(rendered[env]);
};
const doc = (docs, kind) => docs.find((d) => d.kind === kind);
const container = (docs) => doc(docs, "Deployment").spec.template.spec.containers[0];
const check = (docs, env = "staging") => checkOverlay(env, docs, ENVIRONMENTS[env], RANGES);

test("the repository's overlays, cluster config, image and workflow pass", () => {
  if (renderError) throw renderError;
  const { errors, warnings } = checkRepo(root);
  assert.deepEqual(errors, []);
  for (const w of warnings) assert.match(w, /not pinned yet/);
});

test("both rendered overlays pass", () => {
  assert.deepEqual(check(copy("staging")), []);
  assert.deepEqual(check(copy("production"), "production"), []);
  assert.deepEqual(checkParity(copy("staging"), copy("production")), []);
});

test("committed ranges: one per line, comments ignored", () => {
  assert.deepEqual(readRanges("# v4\n1.2.3.0/24\n\n4.5.6.0/24 # x\n"), [
    "1.2.3.0/24",
    "4.5.6.0/24",
  ]);
  assert.ok(RANGES.length > 10);
});

test("allow-list drift fails", () => {
  const removed = copy("staging");
  doc(removed, "Middleware").spec.ipAllowList.sourceRange.pop();
  assert.match(check(removed).join("\n"), /allow list differs from deploy\/cloudflare \(missing/);
  const added = copy("staging");
  doc(added, "Middleware").spec.ipAllowList.sourceRange.push("0.0.0.0/0");
  assert.match(check(added).join("\n"), /extra \["0\.0\.0\.0\/0"\]/);
  const none = copy("staging").filter((d) => d.kind !== "Middleware");
  assert.match(check(none).join("\n"), /no Middleware cloudflare-only/);
});

test("an Ingress router without the middleware fails", () => {
  const docs = copy("staging");
  delete doc(docs, "Ingress").metadata.annotations[
    "traefik.ingress.kubernetes.io/router.middlewares"
  ];
  assert.match(check(docs).join("\n"), /routers lack the staging-cloudflare-only@kubernetescrd/);
  const route = copy("staging");
  route.push({
    kind: "IngressRoute",
    metadata: { name: "side", namespace: "staging" },
    spec: { routes: [{ match: "Host(`x`)", middlewares: [] }] },
  });
  assert.match(check(route).join("\n"), /IngressRoute side has a route without cloudflare-only/);
});

test("wrong host, namespace or TLS secret fails", () => {
  const host = copy("staging");
  doc(host, "Ingress").spec.rules[0].host = "api.arena360.cloud";
  assert.match(check(host).join("\n"), /hosts are \["api\.arena360\.cloud"\]/);
  const ns = copy("staging");
  doc(ns, "Service").metadata.namespace = "default";
  assert.match(check(ns).join("\n"), /Service arena-cloud-control is in namespace default/);
  const tls = copy("staging");
  doc(tls, "Ingress").spec.tls[0].secretName = "other";
  assert.match(check(tls).join("\n"), /TLS must be origin-tls/);
});

test("args, plaintext env, probes, resources and non-root are enforced", () => {
  const args = copy("staging");
  container(args).args = ["--role", "all"];
  assert.match(check(args).join("\n"), /args are/);
  const plain = copy("staging");
  container(plain).env[1] = { name: "ARENA_CONTROL_DB_TOKEN", value: "x" };
  assert.match(check(plain).join("\n"), /env ARENA_CONTROL_DB_TOKEN is not a secretKeyRef/);
  const from = copy("staging");
  container(from).envFrom = [{ secretRef: { name: "x" } }];
  assert.match(check(from).join("\n"), /uses envFrom/);
  const probe = copy("staging");
  delete container(probe).readinessProbe;
  assert.match(check(probe).join("\n"), /readinessProbe is not GET \/healthz/);
  const res = copy("staging");
  delete container(res).resources.limits;
  assert.match(check(res).join("\n"), /resources\.limits/);
  const root_ = copy("staging");
  container(root_).securityContext.readOnlyRootFilesystem = false;
  doc(root_, "Deployment").spec.template.spec.securityContext.runAsNonRoot = false;
  const out = check(root_).join("\n");
  assert.match(out, /readOnlyRootFilesystem is not true/);
  assert.match(out, /pod runAsNonRoot is not true/);
  const secret = copy("staging");
  secret.push({ kind: "Secret", metadata: { name: "s", namespace: "staging" } });
  assert.match(check(secret).join("\n"), /renders a Secret s/);
});

test("environments may differ only in namespace, host, replicas/resources and tag", () => {
  const prod = copy("production");
  container(prod).args = ["--role", "control", "--listen", "0.0.0.0:9"];
  assert.deepEqual(checkParity(copy("staging"), prod), [
    "staging and production differ beyond namespace, host, replicas/resources, image tag, backup bucket and web origins",
  ]);
  const replicas = copy("production");
  doc(replicas, "Deployment").spec.replicas = 9;
  container(replicas).image = "ghcr.io/arena360/arena-cloud:abc123";
  assert.deepEqual(checkParity(copy("staging"), replicas), []);
});

test("sign-up env: secrets by ref, the provider and this environment's web origins", () => {
  const envOf = (docs, name) => container(docs).env.find((e) => e.name === name);
  assert.equal(
    envOf(copy("staging"), "ARENA_WEB_ORIGINS").value,
    "https://start.staging.arena360.cloud",
  );
  assert.equal(
    envOf(copy("production"), "ARENA_WEB_ORIGINS").value,
    "https://start.arena360.cloud",
  );
  const origin = copy("staging");
  envOf(origin, "ARENA_WEB_ORIGINS").value = "https://start.arena360.cloud";
  assert.match(
    check(origin).join("\n"),
    /env ARENA_WEB_ORIGINS is "https:\/\/start\.arena360\.cloud"/,
  );
  const provider = copy("staging");
  envOf(provider, "ARENA_SMS_PROVIDER").value = "fake";
  assert.match(check(provider).join("\n"), /env ARENA_SMS_PROVIDER is "fake"/);
  const missing = copy("staging");
  container(missing).env = container(missing).env.filter((e) => e.name !== "ARENA_SMS_PROVIDER");
  assert.match(check(missing).join("\n"), /env ARENA_SMS_PROVIDER is missing/);
  const pepper = copy("staging");
  envOf(pepper, "ARENA_OTP_PEPPER").valueFrom = undefined;
  envOf(pepper, "ARENA_OTP_PEPPER").value = "x";
  assert.match(check(pepper).join("\n"), /env ARENA_OTP_PEPPER is not a secretKeyRef/);
  const dropped = copy("staging");
  container(dropped).env = container(dropped).env.filter(
    (e) => e.name !== "ARENA_GUPSHUP_PASSWORD",
  );
  assert.match(check(dropped).join("\n"), /env keys are/);
});

const cronjob = (docs) => docs.find((d) => d.kind === "CronJob");
const cjPod = (docs) => cronjob(docs).spec.jobTemplate.spec.template.spec;
const cjContainer = (docs) => cjPod(docs).containers[0];
const checkCj = (docs, env = "staging") => checkCronJob(env, docs, ENVIRONMENTS[env]);

test("both overlays carry the backup CronJob, each with its own bucket", () => {
  assert.deepEqual(checkCj(copy("staging")), []);
  assert.deepEqual(checkCj(copy("production"), "production"), []);
  const bucket = (docs) =>
    cjContainer(docs).env.find((e) => e.name === "ARENA_BACKUP_S3_BUCKET").value;
  assert.notEqual(bucket(copy("staging")), bucket(copy("production")));
  // The same bucket in both environments fails.
  const shared = copy("production");
  cjContainer(shared).env.find((e) => e.name === "ARENA_BACKUP_S3_BUCKET").value =
    ENVIRONMENTS.staging.bucket;
  assert.match(checkCj(shared, "production").join("\n"), /ARENA_BACKUP_S3_BUCKET is/);
  assert.match(
    checkCj(copy("staging").filter((d) => d.kind !== "CronJob")).join("\n"),
    /no CronJob arena-cloud-control-backup/,
  );
});

test("the CronJob's schedule, time zone, concurrency, deadlines and history are enforced", () => {
  const cases = [
    [(d) => (cronjob(d).spec.schedule = "0 3 * * *"), /schedule is "0 3 \* \* \*"/],
    [(d) => delete cronjob(d).spec.timeZone, /timeZone is undefined/],
    [(d) => (cronjob(d).spec.concurrencyPolicy = "Allow"), /concurrencyPolicy/],
    [(d) => (cronjob(d).spec.startingDeadlineSeconds = 60), /startingDeadlineSeconds/],
    [(d) => (cronjob(d).spec.failedJobsHistoryLimit = 1), /failedJobsHistoryLimit/],
    [(d) => (cronjob(d).spec.successfulJobsHistoryLimit = 9), /successfulJobsHistoryLimit/],
    [(d) => (cronjob(d).spec.jobTemplate.spec.backoffLimit = 6), /backoffLimit/],
    [
      (d) =>
        (cjContainer(d).env.find(
          (e) => e.name === "ARENA_BACKUP_S3_ENDPOINT",
        ).valueFrom.secretKeyRef.optional = true),
      /env ARENA_BACKUP_S3_ENDPOINT is optional/,
    ],
    [(d) => delete cronjob(d).spec.jobTemplate.spec.activeDeadlineSeconds, /activeDeadlineSeconds/],
    [(d) => (cjContainer(d).args = ["--role", "jobs"]), /args is/],
  ];
  for (const [mutate, re] of cases) {
    const docs = copy("staging");
    mutate(docs);
    assert.match(checkCj(docs).join("\n"), re, String(re));
  }
});

test("the CronJob runs the Deployment's image and security context with secret env only", () => {
  const image = copy("staging");
  cjContainer(image).image = "ghcr.io/arena360/arena-cloud:other";
  assert.match(checkCj(image).join("\n"), /is not the Deployment's/);
  const sc = copy("staging");
  cjContainer(sc).securityContext.readOnlyRootFilesystem = false;
  cjPod(sc).securityContext.runAsNonRoot = false;
  const out = checkCj(sc).join("\n");
  assert.match(out, /container securityContext differs/);
  assert.match(out, /pod securityContext differs/);
  const plain = copy("staging");
  const i = cjContainer(plain).env.findIndex((e) => e.name === "ARENA_BACKUP_S3_SECRET_ACCESS_KEY");
  cjContainer(plain).env[i] = { name: "ARENA_BACKUP_S3_SECRET_ACCESS_KEY", value: "x" };
  assert.match(checkCj(plain).join("\n"), /SECRET_ACCESS_KEY is not a secretKeyRef/);
  const missing = copy("staging");
  cjContainer(missing).env = cjContainer(missing).env.filter(
    (e) => e.name !== "ARENA_ALERT_WEBHOOK_URL",
  );
  assert.match(checkCj(missing).join("\n"), /keys from arena-cloud-backup are/);
  const envName = copy("staging");
  cjContainer(envName).env.find((e) => e.name === "ARENA_ENV").valueFrom.fieldRef.fieldPath =
    "metadata.name";
  assert.match(checkCj(envName).join("\n"), /ARENA_ENV is not the pod's namespace/);
  const from = copy("staging");
  cjContainer(from).envFrom = [{ secretRef: { name: "arena-cloud-backup" } }];
  assert.match(checkCj(from).join("\n"), /uses envFrom/);
});

test("the cluster config must keep the source IP", () => {
  assert.deepEqual(checkCluster(read("deploy/k8s/cluster/traefik-config.yaml")), []);
  const cluster = read("deploy/k8s/cluster/traefik-config.yaml").replace(
    /^(\s+)externalTrafficPolicy: Local\s*$/m,
    "$1externalTrafficPolicy: Cluster",
  );
  assert.deepEqual(checkCluster(cluster), [
    "cluster: Traefik service externalTrafficPolicy is not Local",
  ]);
});

test("the Dockerfile rules", () => {
  const ok = read("deploy/docker/arena-cloud.Dockerfile");
  assert.deepEqual(checkDockerfile(ok, "1.95.0"), []);
  assert.match(checkDockerfile(ok, "1.96.0").join("\n"), /not rust:1\.96\.0/);
  assert.match(
    checkDockerfile(ok.replace("cc-debian12:nonroot", "cc-debian12:latest"), "1.95.0").join("\n"),
    /not distroless\/cc pinned by tag/,
  );
  assert.match(
    checkDockerfile(ok.replace("USER 65532:65532", "USER root"), "1.95.0").join("\n"),
    /not non-root/,
  );
  assert.match(checkDockerfile(ok.replace("--locked ", ""), "1.95.0").join("\n"), /--locked/);
});

test("the workflow: triggers, tool versions, secrets and smoke steps", () => {
  const ok = read(".github/workflows/deploy-cloud.yml");
  assert.deepEqual(checkWorkflow(ok).errors, []);
  const cases = [
    [ok.replace("branches: [master]", "branches: [main]"), /push\.branches/],
    [
      ok.replace("uses: ./.github/workflows/ci.yml", "uses: ./.github/workflows/other.yml"),
      /calls \.\/\.github\/workflows\/ci\.yml/,
    ],
    [ok.replace("    needs: ci\n", ""), /build does not need ci/],
    [
      ok.replaceAll("        if: ${{ !startsWith(github.ref, 'refs/tags/v') }}\n", ""),
      /must not rebuild or push/,
    ],
    [
      ok.replace("docker manifest inspect", "docker image inspect"),
      /verify the staged image exists/,
    ],
    [
      ok.replace(
        "image: ${{ steps.digest.outputs.image }}",
        "image: ${{ steps.image.outputs.tagged }}",
      ),
      /resolved to its digest/,
    ],
    [
      ok.replace("IMAGE: ${{ needs.build.outputs.image }}", "IMAGE: ghcr.io/x/y:latest"),
      /deploy IMAGE/,
    ],
    [
      ok.replace(
        'kubectl apply -f - < "$RUNNER_TEMP/rendered.yaml"',
        "kubectl apply -f - < deploy/k8s/cluster/traefik-config.yaml",
      ),
      /one-time operator steps/,
    ],
    [ok.replace('tags: ["v*"]', 'tags: ["release-*"]'), /push\.tags/],
    [ok.replace('SOPS_VERSION: "3.13.3"', 'SOPS_VERSION: "3.9.0"'), /SOPS_VERSION/],
    [ok.replace('AGE_VERSION: "1.3.2"', 'AGE_VERSION: "1.2.0"'), /AGE_VERSION/],
    [ok.replace("SOPS_SHA256: PIN-ME", "SOPS_SHA256: abc"), /SOPS_SHA256 is not a sha256/],
    [ok.replaceAll("'production' || 'staging'", "'staging' || 'production'"), /deploy environment/],
    [ok.replace("${{ secrets.SOPS_AGE_KEY }}", "${{ vars.SOPS_AGE_KEY }}"), /SOPS_AGE_KEY/],
    [ok.replace("StrictHostKeyChecking yes", "StrictHostKeyChecking no"), /StrictHostKeyChecking/],
    [ok.replace(".gz_control.reachable == true", ".gz_control"), /edge \/healthz smoke/],
    [ok.replace('"301" || "$code" == "308"', '"200"'), /http -> https smoke/],
    [ok.replace('"403"', '"200"'), /direct-origin smoke/],
    [
      ok.replace('select(.kind == "CronJob" and', 'select(.kind == "Job" and'),
      /backup CronJob's image/,
    ],
    [
      ok.replace(
        "      - name: Apply and wait",
        "      - run: arena-cloud ops restore-control\n      - name: Apply and wait",
      ),
      /never run ops restore-control/,
    ],
  ];
  for (const [text, re] of cases) {
    assert.match(checkWorkflow(text).errors.join("\n"), re, String(re));
  }
  const pinned = ok
    .replace("SOPS_SHA256: PIN-ME", `SOPS_SHA256: ${"a".repeat(64)}`)
    .replace("AGE_SHA256: PIN-ME", `AGE_SHA256: ${"b".repeat(64)}`);
  assert.deepEqual(checkWorkflow(pinned), { errors: [], warnings: [] });
});

test("the deployer RBAC: namespaced, scoped to what a deploy applies", () => {
  const ok = read("deploy/k8s/cluster/deployer-rbac.yaml");
  assert.deepEqual(checkRbac(ok), []);
  assert.match(
    checkRbac(
      ok.replace(
        /kind: Namespace\nmetadata:\n  name: production\n/,
        "kind: ConfigMap\nmetadata:\n  name: x\n",
      ),
    ).join("\n"),
    /no Namespace production/,
  );
  assert.match(
    checkRbac(
      ok.replace('verbs: ["get", "list", "watch", "create", "update", "patch"]', 'verbs: ["*"]'),
    ).join("\n"),
    /wildcard/,
  );
  assert.match(
    checkRbac(ok.replace('resources: ["middlewares"]', 'resources: ["other"]')).join("\n"),
    /cannot apply traefik\.io\/middlewares/,
  );
  assert.match(
    checkRbac(ok.replace('resources: ["cronjobs"]', 'resources: ["jobs"]')).join("\n"),
    /cannot apply batch\/cronjobs/,
  );
  assert.match(
    checkRbac(ok.replaceAll('"arena-cloud-backup", ', "")).join("\n"),
    /cannot update Secret arena-cloud-backup/,
  );
  assert.match(
    checkRbac(
      ok +
        "---\napiVersion: rbac.authorization.k8s.io/v1\nkind: ClusterRoleBinding\nmetadata:\n  name: d\n",
    ).join("\n"),
    /cluster-wide/,
  );
  assert.match(
    checkRbac(
      ok.replace(
        /kind: RoleBinding\nmetadata:\n  name: deployer\n  namespace: staging/,
        "kind: RoleBinding\nmetadata:\n  name: other\n  namespace: staging",
      ),
    ).join("\n"),
    /no RoleBinding .* in staging/,
  );
});

test("overlays render no namespaces and keep the preStop sleep", () => {
  const ns = copy("staging");
  ns.push({ apiVersion: "v1", kind: "Namespace", metadata: { name: "staging" } });
  assert.match(check(ns).join("\n"), /renders cluster-scoped Namespace staging/);
  const stop = copy("staging");
  delete container(stop).lifecycle;
  assert.match(check(stop).join("\n"), /preStop\.sleep/);
});

test("ci.yml is callable by the deploy", () => {
  assert.deepEqual(checkCiWorkflow(read(".github/workflows/ci.yml")), []);
  assert.deepEqual(checkCiWorkflow("on:\n  push:\n"), [
    "ci.yml: no workflow_call trigger (deploy-cloud.yml runs CI first)",
  ]);
});

test("a missing kubectl is one clear error", () => {
  const saved = process.env.PATH;
  process.env.PATH = "";
  try {
    assert.throws(() => render(root, "staging"), { message: KUBECTL_MISSING });
  } finally {
    process.env.PATH = saved;
  }
});
