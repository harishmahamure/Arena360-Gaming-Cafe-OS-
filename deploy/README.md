# Deploying arena-cloud

`arena-cloud --role control` runs on a single k3s node (Hostinger VPS, Mumbai) behind
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

| Path                                    | What                                                                                     |
| --------------------------------------- | ---------------------------------------------------------------------------------------- |
| `docker/arena-cloud.Dockerfile`         | Multi-stage image: `rust:<rust-toolchain.toml>` build, `distroless/cc` non-root runtime  |
| `k8s/base/`                             | Deployment `arena-cloud-control`, Service, Traefik Ingress, `cloudflare-only` Middleware |
| `k8s/overlays/{staging,production}/`    | Namespace, host, replicas, resources, image tag                                          |
| `k8s/cluster/deployer-rbac.yaml`        | One-time: Namespaces, and per namespace a `deployer` ServiceAccount, Role, RoleBinding   |
| `k8s/cluster/traefik-config.yaml`       | One-time: k3s Traefik `HelmChartConfig`, `externalTrafficPolicy: Local` (real source IP) |
| `cloudflare/ips-v4.txt`, `ips-v6.txt`   | Cloudflare's published ranges; the Middleware allow list must equal them                 |
| `secrets/<env>.enc.yaml`                | SOPS + age encrypted `arena-cloud-secrets`, `origin-tls` and `ghcr-pull` Secrets         |
| `../.sops.yaml`                         | SOPS creation rules (age recipients per environment)                                     |
| `../.github/workflows/deploy-cloud.yml` | CI, build or promote, decrypt and apply over SSH, rollout, smoke                         |

`pnpm check:deploy` renders both overlays and checks them, the RBAC, the image and the
workflow; `pnpm check:secrets` fails on any plaintext secret. Neither contacts anything.

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
Middleware and its three Secrets in its own namespace, and watch the rollout.

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

### 6. Encrypt the secrets

Each `deploy/secrets/<env>.enc.yaml` holds **all three** Secrets below; `ghcr-pull` is
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

### 7. GitHub environments

Create environments `staging` and `production` (restrict `production` to `v*` tags and add
required reviewers). Secrets per environment:

| Secret                   | Value                                                         |
| ------------------------ | ------------------------------------------------------------- |
| `SOPS_AGE_KEY`           | The environment's age private key (step 5)                    |
| `DEPLOY_HOST`            | The VPS's public IPv4 (also used by the direct-origin smoke)  |
| `DEPLOY_USER`            | `arena-deploy-staging` or `arena-deploy-production` (step 2)  |
| `DEPLOY_SSH_KEY`         | That user's private SSH key (a different key per environment) |
| `DEPLOY_SSH_KNOWN_HOSTS` | `ssh-keyscan -t ed25519 <host>` output, verified out of band  |

### 8. Pin the tool checksums

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
4. Renders the overlay with the image digest, applies it and waits on `kubectl rollout status`.
5. Smoke: `https://<host>/healthz` through the edge reports `migration_version` and
   `reachable: true`; `http://<host>/healthz` answers 301/308 to `https://`; a direct request to
   the origin IP from the runner (outside Cloudflare) gets 403 or no usable connection.

## Rolling back

Once a new migration has committed to `gz_control`, the previous image refuses to start
(`SCHEMA_TOO_NEW`, exit 2): migrations are forward-only. So never `kubectl rollout undo` past a
migration; fix forward with a new commit (staging) and a new tag (production). A rollback is
only safe to an image with the same `migration_version`, as `/healthz` reports.
