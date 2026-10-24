# Deploying arena-cloud

`arena-cloud --role control` (and, nightly, the `--role jobs` backup CronJob) runs on a single k3s node (Hostinger VPS, Mumbai) behind
Cloudflare. `gz_control`, the only cloud database, is a Turso database in `aws-ap-south-1`
(Mumbai), so control-plane data stays in India.

| Environment  | Namespace    | Host                         | Deployed on        |
| ------------ | ------------ | ---------------------------- | ------------------ |
| `staging`    | `staging`    | `api.staging.arena360.cloud` | push to `master`   |
| `production` | `production` | `api.arena360.cloud`         | push of a `v*` tag |

Every deploy first runs the full CI suite (`ci.yml`, called by the workflow). A push to
`master` builds the image and pushes `ghcr.io/<owner>/arena-cloud:<sha>`; a `v*` tag never
rebuilds: it promotes the image staging already ran for that commit and fails if there is
none. Both environments deploy by digest (`@sha256:`).

## What is in the repository

| Path                                    | What                                                                                                                           |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `docker/arena-cloud.Dockerfile`         | Multi-stage image: `rust:<rust-toolchain.toml>` build, `distroless/cc` non-root runtime                                        |
| `k8s/base/`                             | Deployment `arena-cloud-control`, Service, Traefik Ingress, `cloudflare-only` Middleware, CronJob `arena-cloud-control-backup` |
| `k8s/overlays/{staging,production}/`    | Namespace, host, replicas, resources, image tag, backup bucket                                                                 |
| `k8s/cluster/deployer-rbac.yaml`        | One-time: Namespaces, and per namespace a `deployer` ServiceAccount, Role, RoleBinding                                         |
| `k8s/cluster/traefik-config.yaml`       | One-time: k3s Traefik `HelmChartConfig`, `externalTrafficPolicy: Local` (real source IP)                                       |
| `cloudflare/ips-v4.txt`, `ips-v6.txt`   | Cloudflare's published ranges; the Middleware allow list must equal them                                                       |
| `secrets/<env>.enc.yaml`                | SOPS + age encrypted `arena-cloud-secrets`, `arena-cloud-backup`, `origin-tls` and `ghcr-pull` Secrets                         |
| `../.sops.yaml`                         | SOPS creation rules (age recipients per environment)                                                                           |
| `../.github/workflows/deploy-cloud.yml` | CI, build or promote, decrypt and apply over SSH, rollout, smoke                                                               |

`pnpm check:deploy` renders both overlays and checks them (including the backup CronJob's
schedule, time zone, `Forbid`, deadlines, history limits, image, security context, secret refs
and per-environment bucket), the RBAC, the image and the workflow; `pnpm check:secrets` fails on any plaintext secret. Neither contacts anything.

The origin lock has two layers:

- **The Hostinger VPS firewall** (the provider's, in front of the VM) admits 80/443 only from
  the Cloudflare ranges, plus SSH. Host ufw is **not** relied on: k3s's ServiceLB publishes
  80/443 through iptables DNAT, so that traffic goes through the FORWARD chain and never meets
  ufw's INPUT rules.
- **Traefik's `ipAllowList`** answers 403 to any non-Cloudflare client that still gets in. It
  sees the real peer address because of `externalTrafficPolicy: Local`.

## Operator steps

None of these run from CI; do them once per environment (or when noted). The workflow never
creates namespaces, RBAC or cluster-wide config.

### 1. Turso: create `gz_control` in Mumbai

```sh
turso db create gz-control-staging --location aws-ap-south-1     # and gz-control-production
turso db show gz-control-staging --url                           # -> ARENA_CONTROL_DB_URL (libsql://…)
turso db tokens create gz-control-staging                        # -> ARENA_CONTROL_DB_TOKEN
```

Check `turso db show` reports the `aws-ap-south-1` location. Never paste the token anywhere but
the encrypted secrets file below. `arena-cloud` creates `schema_migrations` and applies its
migrations itself on first start.

### 2. The k3s node

Install the pinned k3s on the VPS (Ubuntu LTS) and record the version here when it changes
(the README pin table checks this line):

```sh
curl -sfL https://get.k3s.io | INSTALL_K3S_VERSION=v1.33.4+k3s1 sh -
```

The cluster is IPv4 only. Keep the kube-apiserver private: never open 6443 in the provider
firewall. CI reaches the cluster only over SSH. Then, as root with the admin kubeconfig
(`/etc/rancher/k3s/k3s.yaml`, which stays `0600 root`):

```sh
kubectl apply -f deploy/k8s/cluster/traefik-config.yaml    # Traefik keeps the source IP
kubectl apply -f deploy/k8s/cluster/deployer-rbac.yaml     # namespaces + per-namespace deployer
```

Re-apply these by hand only when the files change.

**One SSH user per environment.** Each environment's deploys log in as their own Unix user,
whose kubeconfig is that namespace's `deployer` ServiceAccount, so a staging key can never
change production or anything cluster-wide:

```sh
env=staging                                   # then again with env=production
user="arena-deploy-${env}"
sudo adduser --disabled-password --gecos "" "$user"   # no sudo, no k3s group
sudo install -d -m 0700 -o "$user" -g "$user" "/home/$user/.ssh" "/home/$user/.kube"
# authorized_keys: the public half of DEPLOY_SSH_KEY for this environment only.

# A long-lived token for the ServiceAccount (created here, never committed):
kubectl -n "$env" apply -f - <<EOF
apiVersion: v1
kind: Secret
metadata:
  name: deployer-token
  annotations:
    kubernetes.io/service-account.name: deployer
type: kubernetes.io/service-account-token
EOF
token="$(kubectl -n "$env" get secret deployer-token -o jsonpath='{.data.token}' | base64 -d)"
ca="$(kubectl config view --raw -o jsonpath='{.clusters[0].cluster.certificate-authority-data}')"
sudo -u "$user" tee "/home/$user/.kube/config" >/dev/null <<EOF
apiVersion: v1
kind: Config
clusters:
  - name: k3s
    cluster:
      server: https://127.0.0.1:6443
      certificate-authority-data: ${ca}
users:
  - name: deployer
    user:
      token: ${token}
contexts:
  - name: ${env}
    context: { cluster: k3s, namespace: ${env}, user: deployer }
current-context: ${env}
EOF
sudo chmod 0600 "/home/$user/.kube/config"
```

The `deployer` Role may only get/create/update/patch the Deployment, Service, Ingress,
Middleware, the backup CronJob and its four Secrets in its own namespace, and watch the rollout.

### 3. Provider firewall: only Cloudflare reaches 80/443

In the Hostinger panel (VPS → Firewall), create a firewall for the VPS with:

- allow TCP 22 (SSH) from anywhere (or from your admin range plus GitHub's runner ranges);
- allow TCP 80 and 443 from each range in `deploy/cloudflare/ips-v4.txt` (the cluster is
  IPv4 only, so the v6 list is for the Traefik allow list's completeness only);
- drop everything else.

Do not use host ufw for this: ServiceLB's DNAT bypasses it (see above), so it would look like a
lock without being one.

When Cloudflare changes its ranges (<https://www.cloudflare.com/ips/>), update both
`deploy/cloudflare/*.txt` files and `k8s/base/middleware-cloudflare-only.yaml` together
(`pnpm check:deploy` fails on drift), then the provider firewall rules.

### 4. Cloudflare zone `arena360.cloud`

- DNS: `api` and `api.staging` as **proxied** (orange-cloud) **A** records to the VPS's IPv4
  address only. No AAAA records: the cluster is IPv4 only.
- SSL/TLS mode **Full (strict)**.
- **Always Use HTTPS** on (the deploy smoke checks `http://` answers 301/308 to `https://`).
- SSL/TLS → Origin Server: create a **Cloudflare Origin CA certificate** covering
  `api.arena360.cloud` and `api.staging.arena360.cloud`; it goes into the `origin-tls` Secret
  below.

### 5. age keys

One key pair per environment, on an operator machine:

```sh
age-keygen -o staging.agekey      # prints "Public key: age1…"
age-keygen -o production.agekey
```

Put each public key into `.sops.yaml` (replacing the placeholder recipients) and commit that.
Store each private key file's contents as the `SOPS_AGE_KEY` secret of the matching GitHub
environment, and keep an offline copy with the operators. Never commit a `.agekey` file
(`pnpm check:secrets` fails on age private keys).

### 6. Backup: R2 bucket, age key pair and alert webhook

The nightly CronJob `arena-cloud-control-backup` (02:30 Asia/Kolkata) writes an age-encrypted
dump of `gz_control` to Cloudflare R2, off both Hostinger (the cluster) and Turso (the
database), and keeps the newest 30. Per environment:

- **R2 bucket.** In the Cloudflare dashboard (R2 → Create bucket) create
  `arena360-gz-control-backup-staging` (and `…-production`; the overlays name them, and
  `pnpm check:deploy` fails if both environments share one). Location hint: Asia-Pacific. This
  bucket is **only** for control-plane backups, separate from any branch data bucket. Then R2 →
  Manage API tokens → Create API token with **Object Read & Write** scoped to **that bucket
  only**; it gives `ARENA_BACKUP_S3_ACCESS_KEY_ID` and `ARENA_BACKUP_S3_SECRET_ACCESS_KEY`, and
  the S3 endpoint `https://<account id>.r2.cloudflarestorage.com` is
  `ARENA_BACKUP_S3_ENDPOINT`. The token cannot create or delete buckets.
- **Bucket lock.** That token can still overwrite or delete objects, so a compromised cluster
  could erase the history. In the bucket's settings add a **bucket lock rule**: prefix
  `gz_control/nightly/`, retain objects for **30 days**. Retention is unchanged (the job keeps the
  newest 30): with one backup per night, the 31st-newest object is already at least 30 days old,
  so the job's deletes still succeed. If a delete is refused anyway (a lock longer than 30 days,
  or extra runs on one day), the run fails with `BACKUP_RETENTION_FAILED` and alerts, after the
  new backup was written; fix the rule rather than the job.
- **Backup age key pair** (not the SOPS key from step 5). Two operators, together, on an offline
  machine: `age-keygen -o gz-control-backup-<env>.agekey`. The printed public key (`age1…`) is
  `ARENA_BACKUP_AGE_RECIPIENTS` (comma-separate several to add a second recipient, e.g. one key
  per operator). The private key file is copied to two offline media held by the two operators
  and then deleted from the machine. It never goes into the cluster, the repository, CI or a
  password manager shared with CI; restoring is the only time it is used.
- **Alert webhook.** A Slack incoming-webhook URL for the on-call channel (or a Discord webhook
  URL with `/slack` appended) is `ARENA_ALERT_WEBHOOK_URL`. A failed run posts
  `arena-cloud <env>: task control-backup failed with <CODE>` there; the detail is in the job's
  logs (`kubectl -n <env> logs job/<name>`). The key is optional in the Secret (a missing key
  does not block the pod), but set it in every deployed environment.
- **Heartbeat (dead-man's switch).** Some failures never reach the binary, so it cannot alert on
  them: an OOM kill, the 1800 s deadline kill, an image pull failure, a missing Secret key, the
  CronJob not being scheduled at all. Create a check in a healthchecks-style service
  (healthchecks.io, Better Stack heartbeats, or a self-hosted Healthchecks) with period **24 h**
  and grace **2 h**, so it alerts the on-call channel when no ping arrives for 26 h. Its ping URL
  is `ARENA_BACKUP_HEARTBEAT_URL` (optional in the Secret): the job GETs it after every
  successful backup. A failed ping is logged and does not fail the backup.
- **One attempt per night.** The Job has `backoffLimit: 0`: a failed run alerts once and the next
  night runs as usual. To retry by hand: `kubectl -n <env> create job --from=cronjob/arena-cloud-control-backup backup-manual-$(date +%s)`.

### 6a. Owner sign-up: SMS, pepper, web origins and per-IP limits

The control Deployment sends sign-up codes through Gupshup and reads these from
`arena-cloud-secrets` (step 7): `ARENA_OTP_PEPPER` and `ARENA_GUPSHUP_*`. Two plain values sit in
the manifests and are checked by `pnpm check:deploy`: `ARENA_SMS_PROVIDER=gupshup`, and
`ARENA_WEB_ORIGINS`, the onboarding app's origin in that environment
(`https://start.staging.arena360.cloud` in staging, `https://start.arena360.cloud` in
production; each overlay merges it into the env list by name). A missing or short pepper, a
missing Gupshup variable, no origin or `ARENA_SMS_PROVIDER=fake` makes the pod exit 64 at
start. `ARENA_SMS_GLOBAL_PER_MIN` (default 60) caps SMS sends per minute across all phones; set
it in the Deployment if launch traffic needs more.

- **Pepper.** `openssl rand -hex 32`, one per environment, never reused. Rotating it voids the
  codes in flight (users request a new one) and changes every `phone_tag` in the logs.
- **Gupshup.** An Enterprise SMS account with the DLT registration India requires: the sender
  id (header) and a template for `{#var#} is your Arena 360 code. It expires in 5 minutes.`.
  Its id is `ARENA_GUPSHUP_DLT_TEMPLATE_ID`. Tests and CI never call Gupshup; after the first
  deploy of an environment, sign up once with an operator's phone at the onboarding app and
  confirm the SMS arrives (a live check is an operator step). A provider failure answers 503
  and shows in the logs as `signup code send failed` with a `phone_tag`.
- **Onboarding hosting.** The onboarding app (`apps/onboarding`, Cloudflare Workers) is served
  at `start.arena360.cloud` (staging `start.staging.arena360.cloud`). Its `PUBLIC_API_BASE` is
  the API origin of the same environment (`https://api.arena360.cloud`,
  `https://api.staging.arena360.cloud`); session cookies are host-only on the API host.
- **Per-IP limits (Cloudflare).** The cloud limits sends per phone (5 per hour); per-IP limits
  are Cloudflare rate-limiting rules on the `arena360.cloud` zone (Security → WAF → Rate
  limiting rules), one per host, counting by IP, action **Block** with a 429 for the period:

  | Rule          | Expression (`http.host eq "api.arena360.cloud"` and…)                            | Rate          |
  | ------------- | -------------------------------------------------------------------------------- | ------------- |
  | signup start  | `http.request.method eq "POST" and http.request.uri.path eq "/v1/signup/start"`  | 10 per 10 min |
  | signup verify | `http.request.method eq "POST" and http.request.uri.path eq "/v1/signup/verify"` | 30 per 10 min |
  | signup resend | `http.request.method eq "POST" and http.request.uri.path eq "/v1/signup/resend"` | 10 per 10 min |
  | signin        | `http.request.method eq "POST" and http.request.uri.path eq "/v1/signin"`        | 20 per 10 min |
  | signin verify | `http.request.method eq "POST" and http.request.uri.path eq "/v1/signin/verify"` | 30 per 10 min |
  | signin sms    | `http.request.method eq "POST" and http.request.uri.path eq "/v1/signin/sms"`    | 10 per 10 min |

  The sign-in rules (Story 2.6) sit next to the sign-up ones. Inside the cloud, 5 consecutive
  failed sign-ins (wrong password or second factor, in any mix) pause that account for 15
  minutes, and an SMS sign-in code counts against the same 5-per-phone-per-hour send limit as a
  sign-up code. The per-IP rules stop one address from pausing many accounts or guessing
  across them.

  Repeat them for `api.staging.arena360.cloud`. A Cloudflare block answers before the origin,
  so the onboarding app shows its generic "try again" error for it.

### 6b. Owner MFA key

Owners' authenticator (TOTP) secrets are stored encrypted with AES-256-GCM under
`ARENA_MFA_KEY` (a key of `arena-cloud-secrets`, step 7): standard base64 of 32 random bytes,
`openssl rand -base64 32`, one per environment, never reused. A missing key, or one that does
not decode to 32 bytes, makes the pod exit 64 at start. The key is never logged.

- **Keep it with the database backups' recovery material.** A restored `gz_control` is useless
  for TOTP without the key it was written under: every enrolled owner would get 500 `INTERNAL`
  (logged as `mfa secret did not decrypt`) at regenerate and, from Story 2.6, at sign-in.
- **No rotation yet.** Changing the key makes every stored secret unreadable. Until a
  re-encryption job exists, rotate only together with resetting every owner's enrolment.
- **Per-IP limits.** Add Cloudflare rate-limiting rules like those of step 6a for
  `POST /v1/account/mfa/totp/confirm` and `POST /v1/account/mfa/backup-codes/regenerate`
  (30 per 10 min each); both need a signed-in session.

### 6c. Licence signing key

Each branch licence is an EdDSA (Ed25519) JWT that lives 7 days, signed by the cloud with
`ARENA_LICENCE_SIGNING_KEY` (a PKCS#8 PEM private key) under the kid `ARENA_LICENCE_KID`, both
keys of `arena-cloud-secrets` (step 7). Hubs trust only the public keys compiled into
`arena_proto::licence::LICENCE_VERIFY_KEYS` (two slots: the current key and the next one). The
set ships empty, and an empty set refuses every licence ("no licence verify keys"), so this
step must be done before a release that verifies licences.

1. Generate a key, one per environment, on an operator machine:

   ```sh
   cargo run -p arena-cloud -- ops licence-keygen --kid prod-2026-10 > /tmp/licence.pem
   ```

   The private PEM goes to stdout only. stderr prints `ARENA_LICENCE_KID=<kid>` and the line
   to paste into `LICENCE_VERIFY_KEYS`:
   `VerifyKey { kid: "<kid>", public_key: "<base64url>" },`. Without `--kid` the kid is
   `lk-` plus 12 hex characters of the public key's SHA-256.

2. Put the PEM (as a YAML block scalar) and the kid into `arena-cloud-secrets` (step 7), then
   `shred -u /tmp/licence.pem`.
3. Paste the `VerifyKey` line into `LICENCE_VERIFY_KEYS` in
   `crates/arena-proto/src/licence.rs` and release the Hub with it **before** the cloud signs
   with the key.

- **Rotation.** Generate the next key, ship it as the second slot of `LICENCE_VERIFY_KEYS`,
  wait until Hubs run that release, then switch `ARENA_LICENCE_SIGNING_KEY`/`ARENA_LICENCE_KID`
  and, in a later release, drop the retired slot. Licences live 7 days, so a retired key stops
  mattering a week after the switch.
- **Never the dev key.** `--dev` signs with the committed
  `crates/arena-cloud/dev/fixtures/licence-dev-signing-key.pem` (kid `arena-dev-licence-1`),
  whose public key is compiled in only for tests and the `dev-keys` feature of `arena-proto`.
  The control role exits 64 when `ARENA_LICENCE_SIGNING_KEY` is missing, is not an Ed25519
  PKCS#8 PEM, or is that dev key. The key is never logged.
- **Comp entitlements.** Internal and beta cafés get Basic or Premium limits from
  `arena-cloud ops grant-comp --branch <id> --tier basic|premium --until <YYYY-MM-DD>
--operator <name> --reason <text>`, run against the environment's `ARENA_CONTROL_DB_*`. It
  writes an `ops_audit` row and marks the branch's plan step done; `--until` ends at midnight
  Asia/Kolkata. Exit 64 is a usage error (including a missing `ARENA_CONTROL_DB_*`); exit 1 an unknown
  branch, a past date, a branch on a paid plan (never replaced) or a database failure.

### 7. Encrypt the secrets

Each `deploy/secrets/<env>.enc.yaml` holds **all four** Secrets below; `ghcr-pull` is
mandatory, because the Deployment always references it (without it the first rollout hangs in
`ImagePullBackOff`). Write the plaintext only in a temporary file outside the repository,
encrypt it, then delete the plaintext:

```yaml
# /tmp/staging.yaml (never committed)
apiVersion: v1
kind: Secret
metadata:
  name: arena-cloud-secrets
type: Opaque
stringData:
  ARENA_CONTROL_DB_URL: libsql://gz-control-staging-<org>.aws-ap-south-1.turso.io
  ARENA_CONTROL_DB_TOKEN: <token from step 1>
  # Owner sign-up (step 6a): `openssl rand -hex 32`, a different one per environment.
  ARENA_OTP_PEPPER: <64 hex characters>
  ARENA_GUPSHUP_ENDPOINT: https://enterprise.smsgupshup.com/GatewayAPI/rest
  ARENA_GUPSHUP_USERID: <Enterprise user id>
  ARENA_GUPSHUP_PASSWORD: <its password>
  ARENA_GUPSHUP_SENDER: <DLT sender id>
  ARENA_GUPSHUP_DLT_TEMPLATE_ID: <DLT template id of the sign-up message>
  # Owner MFA (step 6b): `openssl rand -base64 32`, a different one per environment.
  ARENA_MFA_KEY: <44 base64 characters>
  # Licence signing (step 6c): from `ops licence-keygen`, a different one per environment.
  ARENA_LICENCE_KID: <kid>
  ARENA_LICENCE_SIGNING_KEY: |
    <the PEM printed on stdout>
---
apiVersion: v1
kind: Secret
metadata:
  name: arena-cloud-backup
type: Opaque
stringData:
  ARENA_BACKUP_S3_ENDPOINT: https://<account id>.r2.cloudflarestorage.com
  ARENA_BACKUP_S3_ACCESS_KEY_ID: <bucket-scoped token, step 6>
  ARENA_BACKUP_S3_SECRET_ACCESS_KEY: <its secret>
  ARENA_BACKUP_AGE_RECIPIENTS: age1… # public key(s) only, step 6
  ARENA_ALERT_WEBHOOK_URL: https://hooks.slack.com/services/…
  ARENA_BACKUP_HEARTBEAT_URL: https://hc-ping.com/<uuid>
---
apiVersion: v1
kind: Secret
metadata:
  name: origin-tls
type: kubernetes.io/tls
stringData:
  tls.crt: |
    <Cloudflare Origin CA certificate>
  tls.key: |
    <its private key>
---
apiVersion: v1
kind: Secret
metadata:
  name: ghcr-pull
type: kubernetes.io/dockerconfigjson
stringData:
  .dockerconfigjson: '{"auths":{"ghcr.io":{"auth":"<base64 user:read:packages token>"}}}'
```

```sh
cp /tmp/staging.yaml deploy/secrets/staging.enc.yaml
sops --encrypt --in-place deploy/secrets/staging.enc.yaml   # rules from .sops.yaml
shred -u /tmp/staging.yaml
pnpm check:secrets                                           # must pass before committing
```

Each document gets its own `sops` block; `pnpm check:secrets` requires `sops.mac` in every
document and `ENC[...]` for every `data`/`stringData` value. The deploy job fails with a clear
message when `deploy/secrets/<env>.enc.yaml` is missing. To rotate the Turso token:
`sops deploy/secrets/<env>.enc.yaml`, edit, save, commit, deploy.

### 8. GitHub environments

Create environments `staging` and `production` (restrict `production` to `v*` tags and add
required reviewers). Secrets per environment:

| Secret                   | Value                                                         |
| ------------------------ | ------------------------------------------------------------- |
| `SOPS_AGE_KEY`           | The environment's SOPS age private key (step 5)               |
| `DEPLOY_HOST`            | The VPS's public IPv4 (also used by the direct-origin smoke)  |
| `DEPLOY_USER`            | `arena-deploy-staging` or `arena-deploy-production` (step 2)  |
| `DEPLOY_SSH_KEY`         | That user's private SSH key (a different key per environment) |
| `DEPLOY_SSH_KNOWN_HOSTS` | `ssh-keyscan -t ed25519 <host>` output, verified out of band  |

### 9. Pin the tool checksums

`deploy-cloud.yml` installs sops `3.13.3` and age `1.3.2` from pinned release URLs and verifies
each download's sha256. Before the first deploy, replace both `PIN-ME` values (`SOPS_SHA256`,
`AGE_SHA256`) with the sha256 of the exact files at `SOPS_URL` and `AGE_URL`, checked against
the projects' published checksums. Until then the install step fails (and `pnpm check:deploy`
warns).

## What a deploy does

1. Runs CI (`ci.yml`). On `master`: builds `deploy/docker/arena-cloud.Dockerfile` and pushes
   `ghcr.io/<owner>/arena-cloud:<sha>`. On a `v*` tag: checks that image exists (it must have
   gone through staging) without rebuilding. Either way it resolves the image's digest.
2. Installs sops and age (pinned URL + sha256) and opens SSH, with the pinned `known_hosts`, as
   the environment's own deploy user.
3. Decrypts `deploy/secrets/<env>.enc.yaml` straight into `kubectl apply` on the node.
4. Renders the overlay with the image digest (the control Deployment and the backup CronJob),
   applies it and waits on `kubectl rollout status`. A deploy never runs a restore.
5. Smoke: `https://<host>/healthz` through the edge reports `migration_version` and
   `reachable: true`; `http://<host>/healthz` answers 301/308 to `https://`; a direct request to
   the origin IP from the runner (outside Cloudflare) gets 403 or no usable connection.

## Rolling back

Once a new migration has committed to `gz_control`, the previous image refuses to start
(`SCHEMA_TOO_NEW`, exit 2): migrations are forward-only. So never `kubectl rollout undo` past a
migration; fix forward with a new commit (staging) and a new tag (production). A rollback is
only safe to an image with the same `migration_version`, as `/healthz` reports.

## Restore gz_control

The recovery time objective is **4 hours** from "gz_control is lost" to the control role
serving from a restored database. `arena-cloud ops restore-control` restores only into an
**empty** database, verifies every table's row count against the dump's manifest and prints
them. It runs from an operator machine, never from a deploy.

**Drill before GA (an operator step, once, then after any change to this runbook):** run the
steps below against **staging** with the latest production-shaped backup and record the timings
at the end of this section.

1. **Pick the backup.** List the bucket (`aws s3 ls` or `rclone` with the bucket-scoped token,
   endpoint `https://<account id>.r2.cloudflarestorage.com`, region `auto`) and take the newest
   `gz_control/nightly/<UTC timestamp>.sql.age`.
2. **Bring the private key.** The two operators bring the offline copy of
   `gz-control-backup-<env>.agekey` (step 6) to the operator machine. Never copy it to the
   node or into the cluster.
3. **A fresh database in Mumbai:**

   ```sh
   turso db create gz-control-staging-restore --location aws-ap-south-1
   turso db show gz-control-staging-restore --url        # -> ARENA_CONTROL_DB_URL
   turso db tokens create gz-control-staging-restore     # -> ARENA_CONTROL_DB_TOKEN
   ```

4. **Restore** (the bucket variables are the same as the CronJob's; a released `arena-cloud`
   binary or `cargo run --release -p arena-cloud --`):

   ```sh
   ARENA_CONTROL_DB_URL=libsql://gz-control-staging-restore-<org>.aws-ap-south-1.turso.io \
   ARENA_CONTROL_DB_TOKEN=<token> \
   ARENA_BACKUP_S3_ENDPOINT=https://<account id>.r2.cloudflarestorage.com \
   ARENA_BACKUP_S3_BUCKET=arena360-gz-control-backup-staging \
   ARENA_BACKUP_S3_ACCESS_KEY_ID=<id> ARENA_BACKUP_S3_SECRET_ACCESS_KEY=<secret> \
   arena-cloud ops restore-control --from-key gz_control/nightly/<timestamp>.sql.age \
     --identity /media/offline/gz-control-backup-staging.agekey
   ```

   It prints `restored gz_control at schema version N` and each table's verified row count, or
   exits 1 with the reason (`RESTORE_TARGET_NOT_EMPTY`, `SCHEMA_TOO_NEW`,
   `RESTORE_COUNT_MISMATCH` or `RESTORE_FOREIGN_KEY_VIOLATION` naming the table,
   `RESTORE_CONFIG_INVALID`, a decrypt error). After a failure part-way, destroy the
   database and start again from step 3. The age 1.3.2 CLI can also decrypt the file
   (`age -d -i <key> x.sql.age | arena-cloud ops restore-control --from-file - --plaintext`).

5. **Point the environment at it.** Edit `deploy/secrets/<env>.enc.yaml` with `sops`, set
   `ARENA_CONTROL_DB_URL` and `ARENA_CONTROL_DB_TOKEN` in `arena-cloud-secrets` to the new
   database, commit and deploy (or, under incident pressure, `sops --decrypt … | ssh node kubectl
apply -f -` followed by `kubectl -n <env> rollout restart deployment/arena-cloud-control`).
6. **Verify.** `https://<host>/healthz` reports `reachable: true` and `migration_version` equal
   to the restored schema version printed in step 4 (the control role applies nothing on start).
7. **Clean up.** Delete the private key from the operator machine; for a drill, destroy the
   restore database and point staging back.

Record each drill:

| Date | Operators | Backup key | Steps 1–4 | Steps 5–6 | Total (≤ 4 h) |
| ---- | --------- | ---------- | --------- | --------- | ------------- |
|      |           |            |           |           |               |
