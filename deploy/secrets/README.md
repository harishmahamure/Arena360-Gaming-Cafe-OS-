# Encrypted deploy secrets

Every other file here is a SOPS + age encrypted Kubernetes Secret manifest, one per
environment:

| File                  | Holds                                                              |
| --------------------- | ------------------------------------------------------------------ |
| `staging.enc.yaml`    | `arena-cloud-secrets`, `origin-tls` and `ghcr-pull` (all required) |
| `production.enc.yaml` | the same, for production                                           |

The creation rules are in `/.sops.yaml`. How to create, encrypt and rotate them is in
[`../README.md`](../README.md#6-encrypt-the-secrets). `pnpm check:secrets` fails on any file
here (other than this README) without SOPS metadata, so a plaintext file can never be
committed.
