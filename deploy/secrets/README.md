# Encrypted deploy secrets

Every other file here is a SOPS + age encrypted Kubernetes Secret manifest, one per
environment:

| File                  | Holds                                                                                    |
| --------------------- | ---------------------------------------------------------------------------------------- |
| `staging.enc.yaml`    | `arena-cloud-secrets`, `arena-cloud-backup`, `origin-tls` and `ghcr-pull` (all required) |
| `production.enc.yaml` | the same, for production                                                                 |

`arena-cloud-backup` holds the nightly backup's bucket-scoped R2 token and endpoint, the
backup age **public** key(s) and the alert webhook URL. The backup's age **private** key is
never here: two operators keep it offline (see
[`../README.md`](../README.md#6-backup-r2-bucket-age-key-pair-and-alert-webhook)).

The creation rules are in `/.sops.yaml`. How to create, encrypt and rotate them is in
[`../README.md`](../README.md#7-encrypt-the-secrets). `pnpm check:secrets` fails on any file
here (other than this README) without SOPS metadata, so a plaintext file can never be
committed.
