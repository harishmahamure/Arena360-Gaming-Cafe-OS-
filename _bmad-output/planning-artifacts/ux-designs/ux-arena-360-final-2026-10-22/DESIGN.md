---
name: Arena 360
description: Local-first management suite for gaming cafés. One brand ("Cinema Electric") in two moods. Player surfaces are cinematic (real game video and cover art under a dark scrim). Staff surfaces use the same palette toned down (solid panels, no art, denser). Dark-only, plus a high-contrast theme. No blur anywhere.
status: draft
updated: 2026-10-23
colors:
  # ── Cinema Electric (default theme) ──────────────────────────────
  bg: '#09071A'
  surface: '#16122C'
  surface-2: '#201A3D'
  line: '#362D5C'                # decorative hairline only (1.5:1); never an input or focusable boundary
  border-strong: '#8178B5'       # ≥3:1 non-text boundary: 5.0 bg · 4.6 surface · 4.1 surface-2 · 4.6 hub-tile · 4.9 hub-bg
  text: '#F5F2FF'
  text-muted: '#ACA5CC'
  accent: '#FF4FA8'
  on-accent: '#1A0010'
  ok: '#5FE6A0'
  warning: '#FFBE3D'
  on-warning: '#1E1300'
  danger: '#FF5C5C'
  on-danger: '#1A0505'           # 6.5:1 on danger (alert-badge ink)
  info: '#7FB2FF'
  focus-ring-inner: '#FF4FA8'    # 2px, offset 2px — 6.6 bg · 6.0 surface · 5.4 surface-2 · 6.1 hub-tile · 3.0 on a white frame
  focus-ring-outer: '#F5F2FF'    # 2px outside the inner ring — 18.0 bg · 14.9 surface-2; carries the ring over dark media
  scrim-base: '#0A0816'          # rgba(10,8,22,α); see components.scrim and components.tile-scrim
  # Hub / staff mood
  hub-bg: '#0B0920'
  hub-header: '#110E28'
  hub-tile: '#141030'
  hub-line: '#2A2350'            # decorative hairline only
  # Floor-tile states (Hub floor view; also kiosk/owner badges). Ratios on hub-tile.
  state-free: '#5FE6A0'          # 11.6
  state-in-session: '#7FB2FF'    # 8.5
  state-paused: '#5C82BD'        # 4.7 — in-session dimmed; always paired with pause icon + "Paused"
  state-reserved: '#C8A2FF'      # 8.8
  state-order-pending: '#FFBE3D' # 11.1
  state-maintenance: '#8A85A8'   # 5.2
  state-offline: '#FF5C5C'       # 6.1
  state-unassigned: '#ACA5CC'    # 7.9 — dashed outline + "Unassigned"
  # Chips
  chip-rule-bg: '#3D0F2A'
  chip-rule-text: '#FFA3D1'
  chip-offline-bg: '#3D1418'
  chip-offline-text: '#FF9A9A'
  # Overlay HUD toast (solid, never translucent)
  hud-toast-bg: '#1E1606'
  hud-toast-border: '#FFBE3D'
  hud-toast-text: '#F5F2FF'
  # ── High-contrast theme (-hc): no art, no video, no translucency ──
  bg-hc: '#000000'
  surface-hc: '#000000'
  surface-2-hc: '#1A1A1A'
  line-hc: '#FFFFFF'
  border-strong-hc: '#FFFFFF'
  text-hc: '#FFFFFF'
  text-muted-hc: '#E0E0E0'
  accent-hc: '#FFE14D'
  on-accent-hc: '#000000'
  ok-hc: '#3DFF8F'
  warning-hc: '#FFB000'
  on-warning-hc: '#000000'
  danger-hc: '#FF6161'
  on-danger-hc: '#000000'        # 7.1:1 on danger-hc
  info-hc: '#6CB8FF'
  focus-ring-hc: '#FFE14D'       # 4px solid, offset 2px — 16.1:1 on black; thicker than the 2px white outlines it must differ from
  hub-bg-hc: '#000000'
  hub-header-hc: '#000000'
  hub-tile-hc: '#000000'
  hub-line-hc: '#FFFFFF'
  state-free-hc: '#3DFF8F'
  state-in-session-hc: '#6CB8FF'
  state-paused-hc: '#4F8FCC'     # 6.1 on black
  state-reserved-hc: '#E0B0FF'
  state-order-pending-hc: '#FFB000'
  state-maintenance-hc: '#D0D0D0'
  state-offline-hc: '#FF6161'
  state-unassigned-hc: '#E0E0E0' # 15.9 on black; dashed
  chip-rule-bg-hc: '#FFE14D'
  chip-rule-text-hc: '#000000'
  chip-offline-bg-hc: '#FF6161'
  chip-offline-text-hc: '#000000'
  hud-toast-bg-hc: '#000000'
  hud-toast-border-hc: '#FFB000'
  hud-toast-text-hc: '#FFFFFF'
typography:
  # Sora (OFL), bundled as a web font in the shared web UI codebase (Tauri Kiosk/Hub, Kitchen, Owner web, Partner portal, marketing). Sora tops out at weight 800.
  # Kiosk scale — authored for 1920×1080 landscape, viewed from ~60–80 cm. Also used by Pi TV screens.
  kiosk-display:
    fontFamily: Sora
    fontSize: 144px
    fontWeight: '200'
    lineHeight: '0.9'
    letterSpacing: 0.02em
  kiosk-headline:
    fontFamily: Sora
    fontSize: 48px
    fontWeight: '300'
    lineHeight: '1.1'
  kiosk-title:
    fontFamily: Sora
    fontSize: 28px
    fontWeight: '600'
    lineHeight: '1.2'
  kiosk-body:
    fontFamily: Sora
    fontSize: 20px
    fontWeight: '400'
    lineHeight: '1.45'
  kiosk-label:
    fontFamily: Sora
    fontSize: 18px
    fontWeight: '600'
    lineHeight: '1.2'
  kiosk-cta:
    fontFamily: Sora
    fontSize: 24px
    fontWeight: '800'
    lineHeight: '1'
  kiosk-eyebrow:
    fontFamily: Sora
    fontSize: 16px
    fontWeight: '600'
    lineHeight: '1.2'
    letterSpacing: 0.14em
    note: 'Uppercase'
  kiosk-tile-name:
    fontFamily: Sora
    fontSize: 20px
    fontWeight: '800'
    lineHeight: '1'
    letterSpacing: 0.02em
  hud-numeric:
    fontFamily: Sora
    fontSize: 22px
    fontWeight: '600'
    lineHeight: '1'
    note: 'Tabular numerals (tnum) — HUD pill, time chip, grace countdown'
  # Hub / staff scale — dense, 1600×900-class desktop down to phone. Also Kitchen display.
  hub-numeric:
    fontFamily: Sora
    fontSize: 26px
    fontWeight: '800'
    lineHeight: '1.1'
    note: 'Tabular numerals (tnum) — floor-tile timers, currency totals, kitchen seat number'
  hub-title:
    fontFamily: Sora
    fontSize: 18px
    fontWeight: '600'
    lineHeight: '1.3'
  hub-body:
    fontFamily: Sora
    fontSize: 15px
    fontWeight: '400'
    lineHeight: '1.45'
  hub-label:
    fontFamily: Sora
    fontSize: 13px
    fontWeight: '500'
    lineHeight: '1.3'
  hub-micro:
    fontFamily: Sora
    fontSize: 11px
    fontWeight: '700'
    lineHeight: '1.2'
    letterSpacing: 0.06em
    note: 'Uppercase'
  # Web (Owner app, marketing, onboarding, partner portal)
  web-display:
    fontFamily: Sora
    fontSize: 40px
    fontWeight: '300'
    lineHeight: '1.15'
  web-body:
    fontFamily: Sora
    fontSize: 16px
    fontWeight: '400'
    lineHeight: '1.5'
rounded:
  xs: 6px
  sm: 10px
  md: 16px
  lg: 20px
  xl: 24px
  full: 9999px
spacing:
  '1': 4px
  '2': 8px
  '3': 12px
  '4': 16px
  '5': 24px
  '6': 32px
  '7': 40px
  '8': 56px
  '9': 64px
  '10': 80px
  '11': 96px
  '12': 128px
  touch-min: 44px
  # Kiosk (unchanged)
  kiosk-margin: 56px
  # Hub (airy console-dashboard; desktop/tablet, phone overrides where named)
  hub-gutter: 32px
  hub-gutter-phone: 16px
  hub-section-gap: 40px          # between floor zones and between page sections
  hub-panel-padding: 24px        # tile-sheet, cards, settle-bar inset
  hub-tile-padding: 16px         # floor-tile, pos product tile
  hub-header-height: 72px
  hub-rail-width: 88px
  floor-grid-gap: 24px
  floor-grid-gap-phone: 12px
  # Owner app (responsive web / PWA)
  owner-gutter: 40px
  owner-gutter-phone: 16px
  owner-section-gap: 56px
  owner-card-padding: 32px
  owner-card-padding-phone: 24px
  owner-header-height: 64px
  owner-sidebar-width: 256px
  # Kitchen display (tablet landscape)
  kitchen-gutter: 24px
  kitchen-column-gap: 24px
  kitchen-ticket-padding: 16px
  kitchen-ticket-gap: 16px
  kitchen-header-height: 64px
  # Web: marketing (player mood) + onboarding and partner portal (staff mood)
  web-gutter: 56px
  web-gutter-phone: 16px
  web-section-gap: 128px         # marketing sections
  web-section-gap-phone: 64px
  web-app-section-gap: 56px      # onboarding, partner portal
  web-card-padding: 32px
  web-card-padding-phone: 24px
  web-header-height: 80px
  web-header-height-phone: 64px
components:
  focus-ring:
    inner: '2px solid {colors.focus-ring-inner}'
    outer: '2px solid {colors.focus-ring-outer}'
    offset: 2px
    hc: '4px solid {colors.focus-ring-hc}, offset 2px'
  button-primary:
    background: '{colors.accent}'
    foreground: '{colors.on-accent}'
    radius: '{rounded.full}'
    typography: '{typography.kiosk-cta}'
    padding: '24px 40px'
    minHeight: '{spacing.touch-min}'
  button-secondary:
    background: '{colors.surface-2}'
    foreground: '{colors.text}'
    border: '1px solid {colors.border-strong}'
    radius: '{rounded.full}'
  button-warning:
    background: '{colors.warning}'
    foreground: '{colors.on-warning}'
    radius: '{rounded.full}'
  text-field:
    background: '{colors.surface-2}'
    foreground: '{colors.text}'
    placeholder: '{colors.text-muted}'
    border: '1px solid {colors.border-strong}'
    borderFocus: '2px solid {colors.accent} + {components.focus-ring}'
    borderError: '2px solid {colors.danger}'
    errorText: '{colors.danger}'
    radius: '{rounded.sm}'
    minHeight: '{spacing.touch-min}'
  scrim:
    gradient: 'linear-gradient(0deg, rgba(10,8,22,0.92) 0%, rgba(10,8,22,0.15) 55%, rgba(10,8,22,0.15) 100%), linear-gradient(90deg, rgba(10,8,22,0.85) 0%, rgba(10,8,22,0.75) 45%, rgba(10,8,22,0) 70%)'
    copyZone: 'left 45% of width × bottom 50% of height'
  tile-scrim:
    gradient: 'linear-gradient(0deg, rgba(10,8,22,0.85) 0%, rgba(10,8,22,0.85) 25%, rgba(10,8,22,0) 55%)'
  video-control:
    background: '{colors.bg}'
    foreground: '{colors.text}'
    border: '1px solid {colors.border-strong}'
    radius: '{rounded.full}'
    size: '{spacing.touch-min}'
  login-panel:
    background: '{colors.surface}'
    foreground: '{colors.text}'
    border: '1px solid {colors.line}'
    radius: '{rounded.xl}'
  signup-form:
    background: '{colors.surface}'
    foreground: '{colors.text}'
    border: '1px solid {colors.line}'
    radius: '{rounded.xl}'
  pill-nav:
    background: '{colors.surface}'
    border: '1px solid {colors.line}'
    radius: '{rounded.full}'
    typography: '{typography.kiosk-label}'
  pill-nav-item-active:
    background: '{colors.text}'
    foreground: '{colors.bg}'
    radius: '{rounded.full}'
  time-chip:
    background: '{colors.surface}'
    foreground: '{colors.text}'
    typography: '{typography.hud-numeric}'
    radius: '{rounded.full}'
    indicator: '{colors.ok}'
    indicatorLow: '{colors.warning}'
  reconnecting-indicator:
    background: '{colors.surface}'
    foreground: '{colors.text-muted}'
    typography: '{typography.kiosk-label}'
    radius: '{rounded.full}'
  volume-control:
    background: '{colors.surface}'
    foreground: '{colors.text}'
    track: '{colors.border-strong}'
    fill: '{colors.text}'
    radius: '{rounded.full}'
    size: '{spacing.touch-min}'
  messages-bell:
    background: '{colors.surface}'
    foreground: '{colors.text}'
    unreadDot: '{colors.info}'
    radius: '{rounded.full}'
    size: '{spacing.touch-min}'
  call-staff-button:
    background: '{colors.surface}'
    foreground: '{colors.text}'
    border: '1px solid {colors.border-strong}'
    radius: '{rounded.full}'
    typography: '{typography.kiosk-label}'
    minHeight: '{spacing.touch-min}'
  account-menu:
    background: '{colors.surface}'
    foreground: '{colors.text}'
    border: '1px solid {colors.line}'
    radius: '{rounded.xl}'
  account-subnav:
    background: '{colors.surface}'
    foreground: '{colors.text-muted}'
    active: '{colors.text}'
    activeMarker: '3px {colors.text} left bar'
    typography: '{typography.kiosk-label}'
  hero:
    media: 'game video → cover → {colors.bg}'
    overlay: '{components.scrim}'
    title: '{typography.kiosk-display}'
    eyebrow: '{typography.kiosk-eyebrow}'
    copyMaxWidth: 864px
  game-tile:
    radius: '{rounded.lg}'
    media: 'cover art → {colors.surface}'
    overlay: '{components.tile-scrim}'
    label: '{typography.kiosk-tile-name}'
    size: 140x196
  game-tile-featured:
    radius: '{rounded.lg}'
    outline: '3px solid {colors.accent}, offset 3px'
    size: 190x260
  all-games-tile:
    background: '{colors.surface}'
    foreground: '{colors.text}'
    border: '1px solid {colors.border-strong}'
    radius: '{rounded.lg}'
    label: '{typography.kiosk-tile-name}'
  most-played-carousel:
    tile: '{components.game-tile}'
    rankBadge: '{colors.bg} fill, {colors.text} ink, {typography.kiosk-label}'
  launcher-card:
    background: '{colors.surface}'
    foreground: '{colors.text}'
    border: '1px solid {colors.line}'
    radius: '{rounded.lg}'
  filter-rail:
    background: '{colors.surface}'
    foreground: '{colors.text-muted}'
    active: '{colors.text}'
  shop-item-card:
    background: '{colors.surface}'
    border: '1px solid {colors.line}'
    radius: '{rounded.lg}'
  popular-badge:
    background: '{colors.bg}'
    foreground: '{colors.text}'
    border: '1px solid {colors.border-strong}'
    radius: '{rounded.full}'
    typography: '{typography.kiosk-eyebrow}'
  cart:
    background: '{colors.surface}'
    foreground: '{colors.text}'
    border: '1px solid {colors.line}'
    radius: '{rounded.xl}'
    total: '{typography.hud-numeric}'
    hubBackground: '{colors.hub-tile}'
    hubTotal: '{typography.hub-numeric}'
  card:
    background: '{colors.surface}'
    border: '1px solid {colors.line}'
    radius: '{rounded.xl}'
    padding: '{spacing.owner-card-padding}'
    paddingPhone: '{spacing.owner-card-padding-phone}'
    paddingWeb: '{spacing.web-card-padding}'
  plan-picker:
    option: '{components.card}'
    selected: '2px solid {colors.text}'
    expired: '{colors.text-muted} ink + "Expired" tag'
  extend-options:
    background: '{colors.hud-toast-bg}'
    border: '2px solid {colors.hud-toast-border}'
    option: '{components.button-secondary}'
    radius: '{rounded.xl}'
  hud-pill:
    background: '{colors.bg}'
    foreground: '{colors.text}'
    typography: '{typography.hud-numeric}'
    radius: '{rounded.full}'
  hud-toast:
    background: '{colors.hud-toast-bg}'
    foreground: '{colors.hud-toast-text}'
    border: '2px solid {colors.hud-toast-border}'
    title: '{colors.hud-toast-border}'
    radius: '{rounded.xl}'
  lock-screen:
    background: '{colors.bg}'
    countdown: '{typography.kiosk-display}'
  undo-toast:
    background: '{colors.surface-2}'
    foreground: '{colors.text}'
    border: '1px solid {colors.border-strong}'
    action: '{colors.accent}'
    radius: '{rounded.md}'
    typography: '{typography.hub-body}'
    padding: '{spacing.4} {spacing.5}'
  alert-badge:
    background: '{colors.danger}'
    foreground: '{colors.on-danger}'
    radius: '{rounded.full}'
    typography: '{typography.hub-micro}'
  hub-rail:
    background: '{colors.hub-bg}'
    border: '1px solid {colors.hub-line}'
    item-active: '{colors.hub-line}'
    label: '{typography.hub-micro}'
    width: '{spacing.hub-rail-width}'
    itemGap: '{spacing.2}'
  hub-bottom-bar:
    background: '{colors.hub-bg}'
    border-top: '1px solid {colors.border-strong}'
    item-active: '{colors.hub-line}'
    label: '{typography.hub-micro}'
    height: '{spacing.9}'
    items: 'Floor · POS · Players · Shift · More'
  hub-header:
    background: '{colors.hub-header}'
    border-bottom: '1px solid {colors.hub-line}'
    backupStatus: '{typography.hub-label}, {colors.text-muted}'
    height: '{spacing.hub-header-height}'
    padding: '0 {spacing.hub-gutter}'
    itemGap: '{spacing.5}'
  floor-tile:
    background: '{colors.hub-tile}'
    border: '1px solid {colors.border-strong}'
    radius: '{rounded.md}'
    stateBarWidth: 3px
    stateBarColor: 'the tile state token: {colors.state-free} · {colors.state-in-session} · {colors.state-paused} · {colors.state-reserved} · {colors.state-maintenance} · {colors.state-offline}'
    unassignedBorder: '2px dashed {colors.state-unassigned}'
    timer: '{typography.hub-numeric}'
    minHeight: '{spacing.12}'
    padding: '{spacing.hub-tile-padding}'
    gap: '{spacing.floor-grid-gap}'
    gapPhone: '{spacing.floor-grid-gap-phone}'
  floor-tile-console:
    base: '{components.floor-tile}'
    deviceIcon: 'controller glyph, {colors.text-muted}'
    tvNumber: '{typography.hub-label}'
  tile-order-badge:
    background: '{colors.state-order-pending}'
    foreground: '{colors.hub-tile}'
    radius: '{rounded.xs}'
    typography: '{typography.hub-micro}'
  tile-sheet:
    background: '{colors.surface}'
    border: '1px solid {colors.hub-line}'
    radius: '{rounded.xl}'
    padding: '{spacing.hub-panel-padding}'
    sectionGap: '{spacing.5}'
  inline-pin:
    background: '{colors.surface-2}'
    foreground: '{colors.text}'
    border: '1px solid {colors.border-strong}'
    filled: '{colors.text} dot'
    radius: '{rounded.sm}'
  player-lookup:
    field: '{components.text-field}'
    guestAction: '{components.button-secondary}'
    outstanding: '{colors.warning} text on {colors.hub-tile}'
  pos-product-grid:
    tile: '{colors.hub-tile} fill, 1px {colors.border-strong}, {rounded.md}'
    tilePadding: '{spacing.hub-tile-padding}'
    gap: '{spacing.4}'
    price: '{typography.hub-numeric}'
    favourite: '{colors.text} star glyph'
    outOfStock: 'photo dimmed; name + "Out of stock" tag stay ≥4.5:1'
  settle-bar:
    background: '{colors.hub-header}'
    border-top: '1px solid {colors.border-strong}'
    padding: '{spacing.4} {spacing.hub-panel-padding}'
    total: '{typography.hub-numeric}'
    action: '{components.button-primary}'
  payment-split:
    row: '{components.text-field}'
    remainder: '{typography.hub-numeric}'
    remainderOpen: '{colors.warning}'
    remainderZero: '{colors.ok}'
  bill-breakdown:
    background: '{colors.hub-tile}'
    padding: '{spacing.hub-panel-padding}'
    rowGap: '{spacing.3}'
    line: '{typography.hub-body}, amounts tabular, right-aligned'
    total: '{typography.hub-numeric}'
  denomination-counter:
    row: '{components.text-field}'
    subtotal: '{typography.hub-numeric}'
  variance-readout:
    figure: '{typography.hub-numeric}, {colors.text}'
    sign: 'Difference = counted − expected; + over, − short'
    figureGap: '{spacing.6}'
    overThreshold: '{colors.warning} border + "Over threshold" label'
  chip-rule:
    background: '{colors.chip-rule-bg}'
    foreground: '{colors.chip-rule-text}'
    radius: '{rounded.full}'
  chip-offline:
    background: '{colors.chip-offline-bg}'
    foreground: '{colors.chip-offline-text}'
    radius: '{rounded.full}'
  status-badge:
    foreground: 'the state or semantic colour, e.g. {colors.state-free}'
    background: '{colors.hub-tile}'
    border: '1px solid currentColor'
    radius: '{rounded.xs}'
    typography: '{typography.hub-micro}'
  tier-lock:
    foreground: '{colors.text-muted}'
    label: '{typography.hub-label}'
    action: 'underlined {colors.text} link'
  kitchen-ticket:
    background: '{colors.hub-tile}'
    border: '1px solid {colors.border-strong}'
    radius: '{rounded.md}'
    padding: '{spacing.kitchen-ticket-padding}'
    stackGap: '{spacing.kitchen-ticket-gap}'
    seat: '{typography.hub-numeric}'
    ready-accent: '{colors.state-free}'
  metric-figure:
    value: '{typography.hub-numeric}'
    label: '{typography.hub-label}, {colors.text-muted}'
    dataAge: '{typography.hub-micro}, {colors.text-muted}'
    gap: '{spacing.2}'
  command-row:
    background: '{colors.surface}'
    border-bottom: '1px solid {colors.line}'
    padding: '{spacing.4} {spacing.5}'
    status: '{components.status-badge}'
  pairing-code:
    code: '{typography.web-display}, tabular, grouped 4 + 4'
    background: '{colors.surface}'
    border: '1px solid {colors.border-strong}'
    radius: '{rounded.xl}'
    padding: '{spacing.web-card-padding}'
  owner-nav:
    background: '{colors.hub-bg}'
    item-active: '{colors.hub-line}'
    label: '{typography.hub-micro}'
    sidebarWidth: '{spacing.owner-sidebar-width}'
    sidebarBorder: '1px solid {colors.hub-line}'
    sidebarLabel: '{typography.hub-label}'
    tabBarHeight: '{spacing.9}'
    tabBarBorder: '1px solid {colors.border-strong}'
    items: 'Today · Reports · Alerts · Manage · Account'
  tab-control:
    background: '{colors.surface}'
    border: '1px solid {colors.line}'
    radius: '{rounded.full}'
    padding: '{spacing.1}'
    item: 'min-height {spacing.touch-min}, padding 0 {spacing.5}, {colors.text-muted} ink'
    itemActive: '{colors.text} fill, {colors.bg} ink'
  wizard-progress:
    width: 256px
    item: 'min-height {spacing.touch-min}, padding 0 {spacing.3}, radius {rounded.sm}, {colors.text-muted} ink'
    itemCurrent: '{colors.hub-line} fill, {colors.text} ink, 2px {colors.text} step-number ring'
    itemDone: '{colors.text} ink, {colors.ok} filled step number with {colors.on-warning} check'
    stepNumber: '{rounded.full}, 1px {colors.border-strong} ring, tabular'
    skipTag: '{typography.hub-micro}, {colors.text-muted}'
    heading: '{typography.hub-micro} — "Step n of 10"'
  logo-slot:
    content: 'branch logo (only per-branch customisation)'
  ad-banner:
    background: '{colors.surface}'
    border: '1px solid {colors.line}'
---

## Brand & Style

Arena 360 is the operating system of a gaming café. It is what the player sits down in front of and what the counter runs on. The Kiosk owns the PC between sessions and stays out of the way during one. There is one brand, **Cinema Electric**, expressed in two moods.

**Player mood** (Kiosk, Pi/QR TV and phone screens, marketing site) is a premium console dashboard. It uses full-bleed real game video or cover art, a dark scrim, light oversized type, and a single hot-magenta accent that means *go*. The feel is neon LAN-party energy, not a back-office form. **Staff mood** (Hub, Kitchen display, Owner app, Onboarding wizard, Partner portal) is the same palette and typeface toned down: solid dark panels, no art, no video, denser layouts. It has to be calm enough to read under Saturday-rush pressure.

Posture rules that hold everywhere:
- **No live blur.** Frosted glass was explored and dropped by choice. Live blur over video is a GPU cost low-end café PCs should not pay, and readability suffers.
- Motion is subtle: short transitions, no flash.
- Branches customise **the logo only**, never colours or background art.
- Dark is the only theme. A high-contrast theme is the accessibility alternative; owners and managers switch it on per device from Hub settings.

[ASSUMPTION] No light theme is planned for any surface.

Composition references:
- The Cinema direction study at [.working/direction-cinema.html](.working/direction-cinema.html) carries the layout and type cues. Its champagne palette and frosted glass are superseded.
- The palette study is [.working/color-themes-1.html](.working/color-themes-1.html), sections "Cinema Electric" and "High Contrast".
- The ggLeap kiosk screenshots in [imports/](imports/) informed structure only.

Conflict rule: see the EXPERIENCE.md preamble.

## Colors

Dark indigo night with one loud accent. Text-on-flat pairs come from the palette study. Non-text, focus and over-media pairs were computed for this revision (WCAG 2.x relative luminance) and are listed beside the tokens in the frontmatter. Floors: 4.5:1 for text, 3:1 for non-text boundaries and large display text.

- **Night (`{colors.bg}` #09071A).** The canvas behind everything. Also the "solid brand surface" at the end of the media fallback chain.
- **Surface / Surface-2 (#16122C / #201A3D).** Panels, menus, cards and field fills. Solid only; they replace glass.
- **Line (#362D5C) vs Border-strong (#8178B5).** `line` and `hub-line` are decorative hairlines between panels (about 1.3–1.6:1). Any boundary a person must *see to use* uses `{colors.border-strong}`: inputs, PIN cells, secondary buttons, floor tiles, toggles and video controls. It measures 4.1:1 on `surface-2` and 4.6:1 on `surface` and `hub-tile`.
- **Text (#F5F2FF, 18.0:1 on bg) and Text-muted (#ACA5CC, 8.5:1).** The only two ink levels. Muted measures 7.1:1 on `surface-2`.
- **Hot Magenta (`{colors.accent}` #FF4FA8; on-accent #1A0010 = 6.6:1).** The single brand accent. Used for the primary CTA (Play now, Start, Settle), the featured-tile outline, the inner focus ring and the Undo action. Never a state colour, never a decorative fill, never on staff alerts. **Never text over media** and **never link colour inside body text**: it is only 2.7:1 against `text`.
- **Links are always underlined**, in `text` or `accent`. Colour alone never marks a link.
- **Semantic: ok #5FE6A0 · warning #FFBE3D (on-warning #1E1300) · danger #FF5C5C (on-danger #1A0505, 6.5:1) · info #7FB2FF.** Feedback only.
- **Hub tones (`hub-bg` #0B0920, `hub-header` #110E28, `hub-tile` #141030, `hub-line` #2A2350).** A slightly deeper, flatter set for staff density.
- **Floor-tile states.** These are the states the Hub must read at a glance. Contrast is measured on `hub-tile`.

  | State | Hex | Contrast | Extra cue |
  |---|---|---|---|
  | free | #5FE6A0 | 11.6:1 | |
  | in-session | #7FB2FF | 8.5:1 | |
  | paused | #5C82BD | 4.7:1 | the in-session hue dimmed, plus a pause icon |
  | reserved | #C8A2FF | 8.8:1 | |
  | order-pending | #FFBE3D | 11.1:1 | |
  | maintenance | #8A85A8 | 5.2:1 | |
  | offline | #FF5C5C | 6.1:1 | |
  | unassigned | #ACA5CC | 7.9:1 | drawn as a dashed outline |

  State is always colour **plus** a text label. Device-group colour (PRD Floor view) never uses the tile body or state bar. [NOTE FOR UX] Where group colour shows on the floor (zone header, legend) is not decided.
- **Chips.** Dynamic-pricing-rule chip #3D0F2A / #FFA3D1 (8.8:1). Offline chip #3D1418 / #FF9A9A (7.9:1).
- **HUD toast.** Solid #1E1606 with a #FFBE3D border and title (10.8:1) and #F5F2FF body (16.2:1).
- **Focus ring.** A double ring: inner 2px `{colors.focus-ring-inner}`, then outer 2px `{colors.focus-ring-outer}`, offset 2px from the control. One of the two rings holds ≥3:1 on any backing. Over a pure-white frame the magenta inner ring is 3.0:1; on any dark surface the white outer ring is ≥14:1. The double ring is what separates focus from the single magenta outline on `game-tile-featured`.
- **Scrim (`scrim-base` #0A0816).** Mandatory behind all text over video or art. It is verified against the **brightest** plausible frame (#FFFFFF), not the darkest. The hero copy (eyebrow, title, meta, CTAs) must stay inside the scrim's copy zone, the left 45% × bottom 50%. There the composite alpha is ≥0.80 and `text` measures ≥10.4:1 over a white frame. `text-muted` measures ≥4.6:1 there but is not used for hero copy; the eyebrow and meta line use `text`. Game-tile labels sit in the bottom 25% of the tile under `tile-scrim` (α 0.85): 12.2:1 over a white cover.

**High-contrast theme (`*-hc` tokens).**
- Pure black surfaces, white text and hairlines.
- Yellow accent #FFE14D on black, brightened semantics.
- The same state meanings: free #3DFF8F, in-session #6CB8FF, paused #4F8FCC (6.1:1), reserved #E0B0FF, order-pending #FFB000, maintenance #D0D0D0, offline #FF6161, unassigned #E0E0E0 dashed.
- Inverted chips: rule = yellow with black text; offline = red with black text.
- Removes **all** art, video and translucency, and draws 2px white outlines on every interactive surface.
- The focus ring becomes a 4px yellow `{colors.focus-ring-hc}` ring (16.1:1). It is thicker than, and a different colour from, the always-on white outlines.

Every component token resolves to its `-hc` twin in HC.

Avoid: gradients as decoration (the scrims are the only gradients), translucent panels, brand colours chosen by a branch, a second accent.

## Typography

**Sora** (SIL OFL) is bundled as a self-hosted web font in the shared web UI + design-token codebase that every surface renders from. Kiosk and Hub run that UI inside Tauri (WebView2 on Windows; Tauri 2 mobile on Android). Rendering is therefore identical everywhere and never depends on a system font. **Tabular numerals are mandatory** for every timer, countdown and currency amount (`hud-numeric`, `hub-numeric`, and any numeric cell). Currency appears as ₹ in examples and is locale-formatted in the product.

Three ramps:

- **Kiosk (1920×1080).** Also used on Pi TV screens.

  | Role | Size / weight | Use |
  |---|---|---|
  | `kiosk-display` | 144/200 | Hero game title, grace countdown |
  | `kiosk-headline` | 48/300 | Screen titles |
  | `kiosk-title` | 28/600 | Panels |
  | `kiosk-body` | 20/400 | Body text |
  | `kiosk-label` | 18/600 | Pill nav, chips |
  | `kiosk-cta` | 24/800 | Primary buttons |
  | `kiosk-eyebrow` | 16/600 | Uppercase, +0.14em |
  | `kiosk-tile-name` | 20/800 | Over cover art, under `tile-scrim` |

  Light weights appear only at display size. The cinematic contrast is a *thin huge title with a heavy small CTA*.
- **Hub / staff.** Also used by the Kitchen display; the seat number uses `hub-numeric`.

  | Role | Size / weight | Use |
  |---|---|---|
  | `hub-numeric` | 26/800 | Floor-tile time left, totals |
  | `hub-title` | 18/600 | Titles |
  | `hub-body` | 15/400 | Body text |
  | `hub-label` | 13/500 | Secondary tile lines |
  | `hub-micro` | 11/700 | Uppercase; state tags and rail labels |

  Nothing in a dense staff view goes below 11px.
- **Web.** `web-display` 40/300 and `web-body` 16/400 serve the Owner app (responsive web/PWA), onboarding, partner portal and marketing. Owner figures use `hub-numeric`.

Copy is English only. Strings stay externalised so other languages can return later. Do not fit layouts to English string lengths so tightly that +30% breaks them.

## Layout & Spacing

The spacing scale is 4-based: 4 · 8 · 12 · 16 · 24 · 32 · 40 · 56 · 64 · 80 · 96 · 128 (`{spacing.1}` … `{spacing.12}`). Every gap, padding and gutter on every surface comes from this scale; there are no off-scale values. `{spacing.touch-min}` (44px) is the floor for every interactive target on every surface.

**Spacing posture: airy and premium on Hub, Owner app, Kitchen display and Web.** These surfaces read like a console dashboard, not a back-office grid: generous page gutters, wide section gaps, and roomy panel and card padding, so each region reads as one calm block under rush pressure. Density comes from fewer, clearer things on screen, never from tighter spacing. The kiosk keeps its own spacing unchanged. The accepted cost: the Hub floor shows fewer tiles per screen and, at high device counts, scrolls and lets staff collapse zones (behaviour in EXPERIENCE.md State Patterns and Responsive & Platform).

| Surface | Page gutter | Section gap | Panel / card padding | Header height |
|---|---|---|---|---|
| Hub (desktop, tablet) | `{spacing.hub-gutter}` 32 | `{spacing.hub-section-gap}` 40 | `{spacing.hub-panel-padding}` 24 (tiles `{spacing.hub-tile-padding}` 16) | `{spacing.hub-header-height}` 72 |
| Hub (phone) | `{spacing.hub-gutter-phone}` 16 | `{spacing.6}` 32 | `{spacing.4}` 16 | `{spacing.9}` 64 |
| Owner app | `{spacing.owner-gutter}` 40 (phone `{spacing.owner-gutter-phone}` 16) | `{spacing.owner-section-gap}` 56 | `{spacing.owner-card-padding}` 32 (phone `{spacing.owner-card-padding-phone}` 24) | `{spacing.owner-header-height}` 64 |
| Kitchen display | `{spacing.kitchen-gutter}` 24 | `{spacing.kitchen-column-gap}` 24 between columns | `{spacing.kitchen-ticket-padding}` 16 | `{spacing.kitchen-header-height}` 64 |
| Web: marketing | `{spacing.web-gutter}` 56 (phone `{spacing.web-gutter-phone}` 16) | `{spacing.web-section-gap}` 128 (phone `{spacing.web-section-gap-phone}` 64) | `{spacing.web-card-padding}` 32 (phone `{spacing.web-card-padding-phone}` 24) | `{spacing.web-header-height}` 80 (phone `{spacing.web-header-height-phone}` 64) |
| Web: onboarding, partner portal | as marketing | `{spacing.web-app-section-gap}` 56 | `{spacing.web-card-padding}` 32 | `{spacing.web-header-height}` 80 |

Inside a panel, related rows sit `{spacing.3}`–`{spacing.4}` apart and unrelated groups `{spacing.5}`–`{spacing.6}` apart. Buttons that sit side by side keep ≥`{spacing.2}`; destructive actions keep the wider separation set per component (e.g. `tile-sheet`).

- **Kiosk (fixed 1920×1080 landscape; spacing unchanged).**
  - `{spacing.kiosk-margin}` (56px) outer margin.
  - The top bar sits 32px from the top and is 64px tall:
    - Left: `logo-slot`.
    - Centre: `pill-nav`.
    - Right cluster, left to right: `reconnecting-indicator` (only when shown), `volume-control`, `messages-bell`, `call-staff-button`, `time-chip`, account chip.
  - Home:
    - The featured `game-tile` row sits upper-left and ends with an `all-games-tile`.
    - The hero copy block is bottom-left and ≤864px wide (inside the scrim copy zone).
    - `video-control` sits at the bottom-right of the hero.
  - Games, Apps and Shop: left filter rail, search, then the grid, following the ggLeap structure ([imports/ggleap-04-games-library.png](imports/ggleap-04-games-library.png), [imports/ggleap-06-shop.png](imports/ggleap-06-shop.png)).
  - Games also carries a `launcher-card` row and a `most-played-carousel` above the grid.
  - Shop opens `cart` as a right-side panel.
  - The Account page puts `account-subnav` on the left and content on the right ([imports/ggleap-07-account.png](imports/ggleap-07-account.png)).
  - Login: a single panel over full-bleed media ([imports/ggleap-01-login.png](imports/ggleap-01-login.png)), with `video-control` bottom-right.
- **Hub (Windows desktop, Android tablet and phone).**
  - Desktop and tablet:
    - A persistent left icon rail (`{spacing.hub-rail-width}`), a header (`{spacing.hub-header-height}`), and a main area with `{spacing.hub-gutter}` padding.
    - Floor view groups tiles by zone. Zones sit `{spacing.hub-section-gap}` apart; inside a zone the tile-grid gap is `{spacing.floor-grid-gap}` (24px); tiles are ≥128px tall with `{spacing.hub-tile-padding}` inside. Each zone has a header row (zone name, tile count, collapse control) `{spacing.4}` above its grid.
    - POS is a single screen: `pos-product-grid` on the left, `cart` on the right, `settle-bar` along the bottom; the grid and cart sit `{spacing.hub-section-gap}` apart.
    - Sheets (`tile-sheet`) and cards use `{spacing.hub-panel-padding}`.
  - Phone:
    - Same tile grid and same screens, but the rail becomes `hub-bottom-bar` (Floor · POS · Players · Shift · More) so the floor fits two tile columns.
    - Gutter drops to `{spacing.hub-gutter-phone}` and the tile gap to `{spacing.floor-grid-gap-phone}` (12px); touch targets stay ≥44px.
    - [NOTE FOR UX] The breakpoint width, and how POS grid/cart/settle arrange at phone width, are not decided.
- **Kitchen display.** Tablet landscape, four status columns (New → Preparing → Ready → Served) `{spacing.kitchen-column-gap}` apart, with tickets stacked newest-first `{spacing.kitchen-ticket-gap}` apart.
- **Owner app.** One responsive layout from phone to desktop. Desktop: `owner-nav` as a left sidebar (`{spacing.owner-sidebar-width}`) and content up to about 1200px wide with `{spacing.owner-gutter}`. Phone: `owner-nav` as a bottom tab bar (64px) and a single column with `{spacing.owner-gutter-phone}`. Today stacks one branch card per branch, `{spacing.owner-section-gap}` between sections.
- **Web surfaces.** Onboarding and the partner portal are staff mood, content up to about 1200px; onboarding puts `wizard-progress` in a left column beside the step content. Marketing is player mood with full-bleed media sections separated by `{spacing.web-section-gap}`; Pricing uses `tab-control` (Cafés · Partners). [NOTE FOR UX] The marketing site's phone navigation (menu pattern for Product · Pricing · Downloads · Help · FAQ + Get started) is not decided.
- **Thermal print.** Receipts, GST invoices, KOT and Z-report follow the structure in EXPERIENCE.md. Paper width is a branch setting. [NOTE FOR UX] Print typography (printer font vs rasterised Sora), logo treatment and per-width layout are not designed.

## Elevation & Depth

Depth comes from **tone and media, not blur**.
- **Player mood.** Shadows separate elements from moving video:
  - Game tiles: 0 10px 30px black @35%.
  - Account menu and `cart`: 0 16px 40px @35%.
  - HUD toast and `extend-options`: 0 20px 60px @50%, so they read over any game.
- **Staff mood.** No shadows. Hierarchy is `hub-bg` → `hub-header` → `hub-tile` → `surface` (sheets). `undo-toast` separates by its `border-strong` edge, not a shadow.
- **High-contrast.** No shadows; 2px white outlines instead.

`backdrop-filter` is banned on every platform. This is a performance rule for low-end café PCs running WebView2, not a renderer limitation.

## Shapes

Round and console-like. Media always clips to its container's radius.

| Radius | Used for |
|---|---|
| `{rounded.full}` | Buttons, pill nav, tab control, chips, time chip, HUD pill, top-bar icon controls, alert badges |
| `{rounded.lg}` (20px) | Game tiles, shop cards, launcher cards |
| `{rounded.xl}` (24px) | Panels, menus, sheets, HUD toast, cart, cards |
| `{rounded.md}` (16px) | Floor tiles, kitchen tickets, undo toast |
| `{rounded.sm}` (10px) | Rail items, wizard-progress rows, text fields, PIN cells |
| `{rounded.xs}` (6px) | Status tags, order badge |

## Components

Shared:

- **focus-ring.** A double ring (magenta inner, white outer, offset 2px). In HC it is a 4px yellow ring. It is shown on keyboard focus on every focusable element and is never suppressed. Sticky bars (`settle-bar`, `hub-bottom-bar`, `ad-banner`) and the open `tile-sheet` must never cover the focused element (scroll padding equals their height).
- **text-field.** `surface-2` fill and a `border-strong` 1px border, `rounded.sm`, ≥44px tall, with the label above.
  - Focus: a 2px accent border plus `focus-ring`.
  - Error: a 2px danger border, a ⚠ icon and the message below in `danger`. Colour is never the only error marker.
  - Password variant: a trailing show/hide toggle.
- **undo-toast.** A `surface-2` bar with a `border-strong` edge. It shows what happened (with the amount, tabular) and an **Undo** action in accent. Hub: bottom-centre, above `settle-bar` or `hub-bottom-bar`. Kitchen: bottom of the screen.
- **alert-badge.** A red pill with `on-danger` ink: a count in the Hub header, or a short tag ("NEW", "CALL", "TIME UP") on a tile or ticket. It is the persistent visual twin of every alert sound.
- **button-primary / button-secondary / button-warning.**
  - Primary: a magenta pill in `kiosk-cta`, ≥340px wide on the kiosk hero, one per view.
  - Secondary: a `surface-2` pill with a `border-strong` border.
  - Warning: an amber pill, used for Extend in `hud-toast` and `extend-options`.
  - [NOTE FOR UX] Hover, pressed and disabled visuals are not specified for any button, tile or card.

Player mood:

- **scrim / tile-scrim.**
  - The hero scrim is the two-gradient overlay in the frontmatter; all hero copy stays in its copy zone.
  - `tile-scrim` holds α 0.85 over the bottom 25% of each cover and fades out by 55%.
  - Both are omitted only when media is absent (solid fallback) or in HC.
- **video-control.** A 44px round pause/play button (solid `bg`, `border-strong` ring, `text` icon) at the bottom-right of the login and Home hero. Its icon shows the action it will take.
- **login-panel.**
  - A solid `surface` panel, `rounded.xl`, centred over full-bleed muted video.
  - Contents: username and password `text-field`s (password with show/hide), the primary button, an underlined "Forgot password?" link and an underlined "Create account" link.
  - Footer: PC name and version.
  - No language picker, no social login.
- **signup-form.** The same panel shell as `login-panel`. Fields are username, password, phone and name. Below them sits the messaging-consent checkbox with its consent text (a `border-strong` box, ≥44px hit area), then the primary button.
- **pill-nav / pill-nav-item-active.** Home · Games · Apps · Shop in a `surface` pill track. The active item inverts to a `text` fill with `bg` ink. The Shop item is absent on Free.
- **time-chip.** An always-visible pill at the top-right showing a status dot and time left in `hud-numeric`. At ≤10 min the dot turns `warning`, the chip gains a 2px `warning` border and a ⚠ glyph replaces the dot. The change is never colour alone.
- **reconnecting-indicator.** A quiet `surface` pill with `text-muted` "Reconnecting…" and a small static icon. It appears only while the Hub is unreachable.
- **volume-control.** A speaker icon button. It opens a horizontal slider: `border-strong` track, `text` fill and thumb, a 44px thumb hit area.
- **messages-bell.** A bell icon button. Unread messages add an `info` dot plus a count, and the accessible name carries the count.
- **call-staff-button.** A "?" icon with the visible label "Call staff", in a `surface` pill with a `border-strong` edge. After a tap it shows "Staff notified".
- **account-menu.** A `surface` panel containing: balance, Extend, Messages, Account, PC settings, Lock, Log out.
- **account-subnav.** A vertical list: Details · Wallet & packs · Order history · Change password. The active item is `text` with a 3px left bar; the others are `text-muted`.
- **hero.** Game video (muted) → cover → `bg`, under the scrim. Contents: eyebrow in `text`, the `kiosk-display` title, a meta line in `text`, Play now (primary) and **Game details** (`button-secondary`), which opens a `card` panel with genre, age rating and launcher.
- **game-tile / game-tile-featured.**
  - A portrait cover-art tile, 140×196. The featured tile is 190×260 with a 3px magenta outline, offset 3px.
  - The name sits bottom-left in `kiosk-tile-name` over `tile-scrim`.
  - The fallback is `surface` with the name centred.
- **all-games-tile.** The last tile of the featured row: `surface` with a `border-strong` edge, a grid icon and "All games".
- **most-played-carousel.** A horizontal row of `game-tile`s titled "Most played here". Each tile carries a rank badge ("#1", "#2", …) at the top-left, with `bg` fill and `text` ink.
- **launcher-card.** A `surface` card with the launcher's logo (Steam, Epic, Riot), its name, and the line "Use your own account".
- **filter-rail.** A left column of categories and tags. The active item is in `text`, the others in `text-muted`. A search `text-field` sits at the top of the content area.
- **shop-item-card.**
  - Contents: photo, name, price (tabular) and an Add button.
  - `popular-badge` sits at the top-left of the photo when applicable.
  - Out of stock: the photo dims; the name and the "Out of stock" tag stay at full ink.
- **popular-badge.** An uppercase "POPULAR" pill: `bg` fill, `text` ink, `border-strong` edge.
- **cart.**
  - Kiosk: a right-side `surface` panel, `rounded.xl`. It lists item lines with ±1 steppers, bundle components indented, and the total in `hud-numeric`, with the primary "Place order" button at the bottom.
  - Hub POS: the same anatomy on `hub-tile`, with the total in `hub-numeric`.
- **card / plan-picker.**
  - `card` is the generic `surface` container.
  - `plan-picker` lays out plan options as cards. The selected card gets a 2px `text` border. Expired packs show muted ink and an "Expired" tag and cannot be selected.
- **hud-pill.** A small always-on-top pill showing time left or elapsed, and balance. Solid `bg`, never translucent.
- **hud-toast.**
  - A solid amber-bordered panel (~640px) at the top-right.
  - Contents: a title in warning amber, a body in `hud-toast-text`, and actions (Extend = `button-warning`, Dismiss = `button-secondary`).
  - It prints the Extend hotkey as key caps: **Ctrl + Shift + E**.
- **extend-options.** The expanded-HUD panel in the `hud-toast` palette. It lists time options with live prices as `button-secondary` rows, plus the payment source (wallet, pack or credit).
- **lock-screen.** Variants:
  - (a) Grace countdown: `kiosk-display` digits over the still-visible game behind the scrim, with an **Extend** `button-warning` beside the countdown.
  - (a′) Fullscreen-exclusive warning at 5 and 1 min: a brief lock-screen-style banner (solid `bg`, `kiosk-headline` "5 min left", Extend key caps) forced into view, or the game minimised/flashed, so the warning is never sound-only.
  - (b) Solid `bg` lock with a "Pay at counter" or "Time's up" message.
  - (c) Solid `bg` lock held over the paused game for staff, when the branch chooses that time-up mode.
  - (d) Forced-end message.
- **logo-slot.** The branch logo in the kiosk top-left and on Pi screens. It is the only branch-specific visual.

Staff mood:

- **hub-rail / hub-bottom-bar.** The left icon rail (`{spacing.hub-rail-width}`) has `hub-micro` labels; the active item is filled `hub-line`. On phone it becomes a 64px bottom bar with five labelled items, Floor · POS · Players · Shift · More, and a `border-strong` top edge. More opens a sheet listing the remaining destinations.
- **hub-header.** Contents, in order:
  - Branch name.
  - The active **chip-rule** (dynamic pricing rule).
  - Connectivity: **chip-offline** when internet or cloud is unreachable, with text naming which.
  - On paid tiers, cloud backup status in `hub-label` muted: last upload, and pending batches when non-zero.
  - The `alert-badge` count.
  - Clock.
  - Signed-in staff.
- **floor-tile.**
  - Body: `hub-tile` with a `border-strong` 1px edge.
  - Top row: device name plus a `status-badge`.
  - Middle: time left or elapsed in `hub-numeric`.
  - Bottom: player name plus a secondary line in `hub-label`.
  - A 3px state-colour bar runs along the bottom edge.
  - Paused: `state-paused` bar and badge, a pause icon, and the timer frozen.
  - Unassigned: a 2px dashed `state-unassigned` border with no state bar.
  - `tile-order-badge` and `alert-badge` sit at the top-right corner. They are not separate tap targets; tapping the tile opens the sheet at the relevant section.
- **floor-tile-console.** A `floor-tile` with a controller glyph before the device name and "TV n" in the secondary line. Its states and timer are identical to a PC tile.
- **tile-order-badge.** A small amber tag in `hub-micro` with `hub-tile` ink (11.1:1). It reads "ORDER" or "READY".
- **tile-sheet.** A `surface` sheet with the tile's actions: start · extend · order · transfer · pause/resume · lock · end. It also shows the rate and `bill-breakdown`. End and Lock sit apart from Extend and Order, with ≥8px spacing, in `button-secondary`.
- **inline-pin.** A four-to-six-cell PIN field rendered inside the current sheet or row. Cells are `surface-2` with a `border-strong` edge; a filled cell shows a `text` dot. Radius is `rounded.sm`.
- **player-lookup.** One phone `text-field` with the results directly below it and a "Guest" `button-secondary` beside it. A result row shows the name and phone. Any credit outstanding is shown first, in `warning` text.
- **pos-product-grid.**
  - A category strip above a grid of product tiles (`hub-tile`, `border-strong`, `rounded.md`). Each tile shows a photo, name and tabular price.
  - Favourites carry a star and sort first.
  - Out of stock: the photo dims and the "Out of stock" tag stays readable.
- **settle-bar.** Pinned to the bottom on `hub-header` with a `border-strong` top edge. It shows the total in `hub-numeric`, the payment-method buttons, and Settle (`button-primary`).
- **payment-split.**
  - One row per method used: method label, amount field, and a reference field where needed (UPI: last 4 digits; card: terminal reference).
  - The running remainder is in `hub-numeric`: `warning` while non-zero, `ok` at zero, always with the words "Remaining" or "Paid in full".
- **bill-breakdown.** Line items, left-aligned labels with right-aligned tabular amounts, then a ruled total in `hub-numeric`.
- **denomination-counter.** One row per note or coin: denomination, a count field, and a tabular subtotal. The running total sits at the foot. A "Single total" toggle swaps the rows for one field.
- **variance-readout.**
  - Counted, Expected and Difference as three `hub-numeric` figures, `{spacing.6}` apart. Difference = counted − expected and carries a sign and the word "over" (+, extra cash) or "short" (−).
  - Over threshold: a `warning` border and the label "Over threshold", never red.
  - A "Breakdown" underlined link sits below.
- **chip-rule / chip-offline.** Header pills, per Colors.
- **status-badge.**
  - A small uppercase tag in `hub-micro`: state or semantic colour as text and a 1px border, on `hub-tile` (ratios as in Colors).
  - Also used for command status in the Owner app: pending = info, applied = ok, rejected = danger, expired = text-muted.
- **tier-lock.** The gated control is shown with a lock glyph in `text-muted`, plus "Available on Basic" (or Premium) and an underlined "Upgrade" link. Staff and owner surfaces only.
- **kitchen-ticket.**
  - A `hub-tile` card with a `border-strong` edge.
  - Contents: the seat number in `hub-numeric`, items and modifiers, order id, and elapsed time (tabular).
  - A new ticket carries an `alert-badge` "NEW" until it is advanced.
  - Ready tickets gain a `state-free` edge plus a "READY" tag.
- **metric-figure.** An Owner app figure: `hub-label` muted label, `hub-numeric` value, and data age in `hub-micro` muted below ("as of 2 min ago"). On Today branch cards, cash variance (counted − expected, signed, "over"/"short") sits beside cash in drawer [ASSUMPTION — confirm at mock walkthrough].
- **owner-nav.** Owner app navigation: Today · Reports · Alerts · Manage · Account. Desktop: a left sidebar (`{spacing.owner-sidebar-width}`) on `hub-bg` with icon + `hub-label` text; the active item is filled `hub-line`. Phone: a 64px bottom tab bar with icon + `hub-micro` label and a `border-strong` top edge. Live view sits in the sidebar (locked below Premium with `tier-lock`).
- **command-row.** An Owner app command line: what changed, the scope (branch or "All branches"), `status-badge`, time queued, and Retry or Resend when applicable.
- **wizard-progress.** The onboarding step list in a left column (~256px): "Step n of 10" in `hub-micro`, then one ≥44px row per step with a round tabular step number. Done steps: `text` ink and an `ok` check; the current step: `hub-line` fill and a 2px `text` ring on its number; later steps: `text-muted`. Skippable steps carry a muted "Can skip" tag.
- **tab-control.** A segmented pill switch for peer views on one page (marketing Pricing: Cafés · Partners). Same anatomy as `pill-nav`: a `surface` track with `{spacing.1}` inset, items ≥44px tall, the selected item inverted to `text` fill with `bg` ink. Not used for site or app navigation.
- **pairing-code.** The onboarding code display: an 8-character code in `web-display` with tabular figures, grouped 4 + 4, beside a QR code and a countdown. It sits in a `surface` card with a `border-strong` edge.
- **ad-banner.** [ASSUMPTION] A plain `surface` strip with an "Ad" label at the bottom of the Free-tier Android Hub. It never overlaps the floor grid, cart or settle bar and never shares an edge with `settle-bar` or `cart`. It never appears on a kiosk in Phase 1.

## Do's and Don'ts

| Do | Don't |
|---|---|
| Real game video or cover art behind player surfaces, always under the scrim | Text directly on raw art or video |
| Keep hero copy inside the scrim copy zone (left 45% × bottom 50%) | Stretch the hero copy block across the frame |
| Verify text over media against a white frame | Verify against the darkest frame |
| Solid panels everywhere | `backdrop-filter`, frosted glass, translucent HUD |
| Fall back video → cover → solid `bg` | Placeholder gradients pretending to be art |
| One magenta accent, for primary action, featured outline, focus inner ring | Magenta for state, alerts, decoration, or text over media |
| `border-strong` on every input, PIN cell and focusable boundary | `line` / `hub-line` as the only edge of something a person must operate |
| Double focus ring (HC: 4px yellow) on every focusable element | Removing outlines, or focus that looks like the featured outline |
| Underline every link | Colour-only links |
| State = colour + label on every floor tile | Colour-only state |
| Tabular numerals for timers and currency | Proportional digits that jitter while counting |
| Branch logo in the logo slot | Branch colours, background art, or fonts |
| Subtle, short transitions; none in high-contrast | Flashy motion, parallax, autoplaying sound |
| Staff mood: no art, no video, denser | Cinematic media on Hub, Kitchen, Owner or Partner surfaces |
| 44px minimum targets on every surface | Shrinking Hub phone targets to fit more tiles |
| On-scale spacing tokens; airy gutters and padding on Hub, Owner, Kitchen and Web | Off-scale gaps (10px, 20px) or tightening spacing to fit more tiles |
