# Cloudflare Tunnel cost spike (Story 8.39)

The build spike asked for the per-branch cost of a Cloudflare Tunnel plus its Access application,
set against the Rs 200 a month Tunnel add-on (Premium includes the tunnel). This is the record of
that spike. It is a cost model, not a measurement: nothing here was billed. The prices below are the
published list prices as the spike author knew them and **must be re-read from the Cloudflare
dashboard and price list when the production account is opened**; the arithmetic is what carries over.

## What one entitled branch creates

| Object | Made by | Billed |
|--------|---------|--------|
| 1 named tunnel `arena-<branch_id>` (remotely managed, ingress set through the API) | `TunnelProvider::ensure` | No charge for tunnels or their traffic |
| 1 proxied CNAME `<branch_id>.tunnel.arena360.cloud` to `<tunnel id>.cfargotunnel.com` | `TunnelProvider::ensure` | No charge for a DNS record |
| 1 Access application (self-hosted) on the same hostname, one reusable policy | `TunnelProvider::ensure` | Per **seat** (see below), not per application |
| 1 `cloudflared` connector, outbound only, on the branch PC | the Windows Hub | The PC's own bandwidth |

In the steady state a heartbeat answers `on` from the control database and makes **no** Cloudflare
call, so the API volume is two to eight calls when a branch is entitled or loses the flag, and none
otherwise.

## Where the money is: Access seats

Tunnels, DNS and bandwidth through the tunnel are free. Access is the only line that scales with
customers, and it is priced per distinct user who signs in to any Access application in the
billing month (a seat), across the whole account. An owner with three branches is one seat.

Assumptions: Zero Trust free plan includes 50 seats; beyond that a seat is USD 7 a month; USD 1 =
Rs 85.

| | Per month |
|-|-----------|
| Tunnel add-on price | Rs 200 (about USD 2.35) |
| One Access seat past the free 50 | USD 7 (about Rs 595) |
| Tunnel + DNS + connector | USD 0 |

Reading it per branch, with `s` the number of distinct people who open live view of that branch in a
month and no free seats left:

- cost per branch = `s` x Rs 595
- the Rs 200 add-on covers **none** of a seat: even one owner who opens live view costs about three
  times the add-on price
- the first 50 distinct viewers across all branches cost nothing; the account is profitable on the
  add-on alone only while it stays inside that free allowance, or while Premium owners (who pay for
  the whole plan) carry the cost

## Decisions this leads to

1. **Keep Access off the device path.** Remote kiosks (Story 8.41) authenticate with their own
   device token at the Hub, so their paths should be a Bypass policy in the Access application and
   cost no seats. Only the owner's live view (Story 8.40) needs an Access identity, and a single
   shared service token for it would also avoid seats: choose between the two before 8.40.
2. **One policy, many applications.** `ARENA_CF_ACCESS_POLICY_ID` attaches one reusable policy to
   every branch application, so a policy change is one edit, not one per branch.
3. **Watch the seat count, not the tunnel count.** Alert at 40 distinct Access users in a month
   (the free allowance is 50), and re-price the add-on or move to the service token before the 51st.
4. **Account limits.** Tunnel and Access application counts per account have documented limits
   (a few thousand tunnels at the time of writing). They are far above the first year's branch
   count; re-read them when the account is opened.

## Left open

- Real prices and the free allowance, from the production Cloudflare account.
- Whether the owner's live view uses per-user Access identities or a service token (decision 1).
- Egress: live view is a read-only stream of session ticks; its volume per branch is small and has
  not been measured. Measure it with Story 8.40's first end-to-end run.
- Certificate cover for `<branch_id>.tunnel.arena360.cloud`. It is two labels under the zone, and
  Cloudflare's free Universal certificate covers only `*.arena360.cloud`. Check this against the
  account before 8.40: if a proxied hostname at this depth needs Advanced Certificate Manager,
  its monthly price is a cost per zone (not per branch) to add to this model, or the name moves to
  one label (`<branch_id>-tunnel.arena360.cloud`).
- Cleaning up after a Hub that is revoked or gone for good. The cloud deletes a tunnel when the
  flag is removed or the Hub is Android, both seen in a heartbeat; a revoked Hub sends none, so
  an operator sweep of `branch_tunnels` for revoked Hubs is still to be built.
