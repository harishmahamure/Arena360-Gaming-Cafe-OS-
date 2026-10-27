# Ad category blocks for the Hub app's AdMob account

This note is the repo's record of what is blocked in the AdMob account that serves the Arena 360
Hub app (Story 4.12). Change the account and this note together, and raise the version.

Config version: 1

## Blocked at app level

AdMob console → Blocking controls → the Hub app → Sensitive categories. These stay blocked for
every Hub, whatever content rating a branch chooses:

- Adult / sexual
- Gambling and betting
- Alcohol

## Per-branch content rating

Set in the Hub on the device (Settings → Branch → Ads): G, PG (default) or T. The Kotlin plugin
passes it to the SDK as the request configuration's maximum ad content rating. MA is never offered
and a stored MA or unknown value is PG.

## Release check

A named person opens the AdMob console before each release, confirms the three categories above
are blocked for the Hub app, and fills in the three lines below. `node scripts/check-ads.mjs note
--release` fails until they are filled in for the release being cut.

Checked by: (nobody yet)
Checked on: (not yet)
Checked for release: (not yet)
