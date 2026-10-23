---
name: Arena 360
description: Local-first management suite for gaming cafés. One brand ("Cinema Electric") in two moods. Player surfaces are cinematic (real game video and cover art under a dark scrim). Staff surfaces use the same palette toned down (solid panels, no art, denser). Kiosk, Hub, Kitchen display and Pi screens are dark-only plus a high-contrast theme; the Owner app and all web surfaces also offer a light theme ("Cool paper"). No blur anywhere.
status: final
updated: 2026-10-23
colors:
  # ── Cinema Electric (default theme) ──────────────────────────────
  bg: '#09071A'
  surface: '#16122C'
  surface-2: '#201A3D'
  line: '#362D5C'                # decorative hairline only (1.6 bg · 1.5 surface · 1.3 surface-2); never an input or focusable boundary
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
  # ── Light theme "Cool paper" (-light): Owner app + web only. Ratios computed (WCAG 2.x); none adjusted ──
  bg-light: '#F6F5FC'
  surface-light: '#FFFFFF'
  surface-2-light: '#ECEAF7'
  line-light: '#DCD8EE'            # decorative hairline only (1.3 bg · 1.4 surface · 1.2 surface-2)
  border-strong-light: '#7A72A8'   # ≥3:1 boundary: 4.0 bg · 4.4 surface · 3.7 surface-2
  text-light: '#16123A'            # 16.4 bg · 17.8 surface · 15.0 surface-2
  text-muted-light: '#555075'      # 7.0 bg · 7.5 surface · 6.3 surface-2
  accent-light: '#D4127A'          # fills only; 4.7 bg · 5.0 surface · 4.2 surface-2 (non-text)
  on-accent-light: '#FFFFFF'       # 5.0 on accent-light
  accent-text-light: '#AD0E63'     # links, rule-chip-adjacent links: 6.4 bg · 7.0 surface · 5.9 surface-2 · 5.8 chip-rule-bg · 5.8 chip-offline-bg
  ok-light: '#0E7A46'              # 5.0 bg · 5.4 surface · 4.5 surface-2
  warning-light: '#FFBE3D'         # fill only (1.5 on bg, so always edged; see button-warning)
  warning-text-light: '#8F5300'    # warning ink and edges: 5.7 bg · 6.2 surface · 5.2 surface-2
  on-warning-light: '#1E1300'      # 11.1 on warning-light
  danger-light: '#C21F2E'          # 5.5 bg · 6.0 surface · 5.0 surface-2
  on-danger-light: '#FFFFFF'       # 6.0 on danger-light
  info-light: '#1F56C2'            # 6.1 bg · 6.6 surface · 5.6 surface-2
  focus-ring-light: '#AD0E63'      # 2px, offset 2px: 6.4 bg · 7.0 surface · 5.9 surface-2
  chip-rule-bg-light: '#FCE4F0'
  chip-rule-text-light: '#940B54'  # 7.2 on chip-rule-bg-light
  chip-offline-bg-light: '#FDE6E6'
  chip-offline-text-light: '#A3141F' # 6.6 on chip-offline-bg-light
  metric-figure-light: '#16123A'
  state-free-light: '#0E7A46'      # status ink; ≥4.5 on bg, surface and surface-2 (lowest 4.5 on surface-2)
  state-in-session-light: '#1F56C2'
  state-paused-light: '#4A5F8C'
  state-reserved-light: '#6B3FC4'
  state-order-pending-light: '#8F5300'
  state-maintenance-light: '#625E78'
  state-offline-light: '#C21F2E'
  state-unassigned-light: '#555075'
  # ── Device-group swatches (owner picks one per group; dot only, never a fill) ──
  group-teal: '#3FD0D4'            # ratios on hub-tile / black: 9.8 / 11.2
  group-orange: '#FF8C42'          # 7.9 / 9.1
  group-lime: '#C4E35A'            # 12.6 / 14.5
  group-sand: '#E0B27A'            # 9.4 / 10.8
  group-coral: '#FF8FA3'           # 8.5 / 9.7
  group-silver: '#E6E3F5'          # 14.5 / 16.7
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
  web-hero:
    fontFamily: Sora
    fontSize: 64px
    fontWeight: '200'
    lineHeight: '1.0'
    note: 'Marketing and onboarding heroes at ≥1024px; below that the hero uses web-display'
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
  # Kiosk
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
    light: '2px solid {colors.focus-ring-light}, offset 2px'
  interaction-states:
    hover: 'surface 6% lighter; accent fills 8% lighter (light theme: 6% darker)'
    pressed: '6% darker + 1px inset'
    disabled: '40% opacity, no hover, not-allowed cursor, reason text nearby at full ink'
    hc: 'hover = 2px outline thickens to 3px; pressed = outline inset; focus ring stays separate'
  theme-toggle:
    base: '{components.tab-control}'
    items: 'System · Light · Dark'
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
    borderLight: '1px solid {colors.warning-text-light}'
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
    controllerCheck: '{components.status-badge} "CHECK CONTROLLERS" in {colors.warning}'
  group-chip:
    background: '{colors.surface-2}'
    foreground: '{colors.text-muted}'
    border: '1px solid {colors.hub-line}'
    radius: '{rounded.full}'
    typography: '{typography.hub-micro}'
    dot: '8px circle in one of {colors.group-teal} · {colors.group-orange} · {colors.group-lime} · {colors.group-sand} · {colors.group-coral} · {colors.group-silver}'
    padding: '{spacing.1} {spacing.2}'
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
  phone-cart-bar:
    background: '{colors.hub-header}'
    border-top: '1px solid {colors.border-strong}'
    label: '{typography.hub-title}, tabular amount'
    height: '{spacing.9}'
    sheet: 'full-height sheet: {components.cart} + {components.settle-bar}'
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
  kitchen-top-bar:
    background: '{colors.hub-header}'
    border-bottom: '1px solid {colors.hub-line}'
    height: '{spacing.kitchen-header-height}'
    label: '{typography.hub-title}'
    openCount: '{typography.hub-label}, tabular'
    lanStatus: '{components.status-badge}'
    muteToggle: '{components.button-secondary}, size {spacing.touch-min}'
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
    copyAction: '{components.button-secondary}'
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
  ad-interstitial:
    background: '{colors.bg}'
    label: '"Ad" {typography.hub-micro}, {colors.text-muted}'
    skip: '{components.button-secondary}, top-right, countdown "Skip in 5 s" then "Skip ad"'
  tv-idle:
    media: 'muted game video → cover → {colors.bg}, under {components.scrim}'
    logo: '{components.logo-slot}'
    tvNumber: '{typography.kiosk-headline}'
    rates: '{typography.kiosk-title}, tabular'
    qr: '≥360px square, black on white quiet zone, {rounded.md} white frame'
    qrLabel: '{typography.kiosk-cta} "Scan to start"'
  tv-warning:
    background: '{colors.bg}'
    border: '8px solid {colors.warning}'
    headline: '{typography.kiosk-display}, tabular'
    message: '{typography.kiosk-headline}, {colors.text}'
  tv-lock:
    background: '{colors.bg}'
    headline: '{typography.kiosk-headline}, {colors.text}'
    logo: '{components.logo-slot}'
  qr-start-page:
    background: '{colors.bg}'
    header: '{components.logo-slot} + "TV n" {typography.kiosk-title}'
    option: '{components.plan-picker}'
    action: '{components.button-primary}'
    padding: '{spacing.web-gutter-phone}'
  live-timer:
    background: '{colors.surface}'
    border: '1px solid {colors.line}'
    radius: '{rounded.xl}'
    figure: '{typography.kiosk-display} at 96px, tabular'
    label: '{typography.kiosk-label}, {colors.text-muted}'
    note: '{typography.kiosk-body}, {colors.text}'
---

## Brand & Style

Arena 360 is the operating system of a gaming café. It is what the player sits down in front of and what the counter runs on. The Kiosk owns the PC between sessions and stays out of the way during one. There is one brand, **Cinema Electric**, expressed in two moods.

**Player mood** (Kiosk, Pi/QR TV and phone screens, marketing site) is a premium console dashboard. It uses full-bleed real game video or cover art, a dark scrim, light oversized type, and a single hot-magenta accent that means *go*. The feel is neon LAN-party energy, not a back-office form. **Staff mood** (Hub, Kitchen display, Owner app, Onboarding wizard, Partner portal) is the same palette and typeface toned down: solid dark panels, no art, no video, denser layouts. It has to be calm enough to read under Saturday-rush pressure.

Posture rules that hold everywhere:
- **No live blur.** Frosted glass was explored and dropped by choice. Live blur over video is a GPU cost low-end café PCs should not pay, and readability suffers.
- Motion is subtle: short transitions, no flash.
- Branches customise **the logo only**, never colours or background art.
- Kiosk, Hub, Kitchen display and Pi/QR screens are dark only. A high-contrast (HC) theme is the accessibility alternative; owners and managers switch it on per device from Hub settings.
- The Owner app and all web surfaces (marketing, pricing, onboarding, partner portal) also have a light theme, **Cool paper**. See Colors, Light theme.

### Visual references

The spines (this file and `EXPERIENCE.md`, the two source-of-truth specs) win on conflict with any mock, wireframe, or import.

- [Key-screen index](mockups/mocks-index.html): the 19 key-screen mocks (1:1, Cinema Electric, no blur). Each is linked from the section it illustrates below.
- [Colour themes](mockups/color-themes.html): palette source. Only sections "4 · Cinema Electric" and "5 · High Contrast" are adopted; Champagne, Glacier and Ember were set aside.
- [Light theme variants](mockups/color-themes-light.html): light palette source. Only variant "1 · Cool paper" is adopted; Warm paper and Lavender wash were set aside.
- [Cinema direction](mockups/direction-cinema.html): layout and type cues only. It **predates the Electric palette and the no-blur decision**; its champagne accent, frosted glass and translucent HUD are superseded.
- [ggLeap kiosk screenshots](imports/): structure only ([login](imports/ggleap-01-login.png), [home](imports/ggleap-02-home.png), [account menu](imports/ggleap-03-account-menu.png), [games](imports/ggleap-04-games-library.png), [apps](imports/ggleap-05-apps.png), [shop](imports/ggleap-06-shop.png), [account](imports/ggleap-07-account.png)).

Mock annotations marked as assumptions or open questions have all been resolved; where a mock and a spine differ, the spine wins. Mocks that predate a later decision are listed in EXPERIENCE.md Visual References.

## Colors

Dark indigo night with one loud accent. Text-on-flat pairs come from the palette study ([colour themes](mockups/color-themes.html), Cinema Electric and High Contrast). Non-text, focus and over-media pairs were computed with WCAG 2.x relative luminance and are listed beside the tokens in the frontmatter. Floors: 4.5:1 for text, 3:1 for non-text boundaries and large display text.

- **Night (`{colors.bg}` #09071A).** The canvas behind everything. Also the "solid brand surface" at the end of the media fallback chain.
- **Surface / Surface-2 (#16122C / #201A3D).** Panels, menus, cards and field fills. Solid only; they replace glass.
- **Line (#362D5C) vs Border-strong (#8178B5).** `line` and `hub-line` are decorative hairlines between panels: `line` measures 1.6:1 on `bg`, 1.5:1 on `surface` and 1.3:1 on `surface-2`; `hub-line` measures 1.1–1.4:1 on the Hub tones. Any boundary a person must *see to use* uses `{colors.border-strong}`: inputs, PIN cells, secondary buttons, floor tiles, toggles and video controls. It measures 4.1:1 on `surface-2` and 4.6:1 on `surface` and `hub-tile`.
- **Text (#F5F2FF, 18.0:1 on bg) and Text-muted (#ACA5CC, 8.5:1).** The only two ink levels. Muted measures 7.1:1 on `surface-2`.
- **Hot Magenta (`{colors.accent}` #FF4FA8; on-accent #1A0010 = 6.6:1).** The single brand accent. Used for the primary CTA (Play now, Start, Settle), the featured-tile outline, the inner focus ring and the Undo action. Never a state colour, never a decorative fill, never on staff alerts. **Never text over media** and **never link colour inside body text**: it is only 2.7:1 against `text`.
- **Links are always underlined**, in `text` or `accent`. Colour alone never marks a link.
- **Semantic: ok #5FE6A0 · warning #FFBE3D (on-warning #1E1300) · danger #FF5C5C (on-danger #1A0505, 6.5:1) · info #7FB2FF.** Feedback only.
- **Hub tones (`hub-bg` #0B0920, `hub-header` #110E28, `hub-tile` #141030, `hub-line` #2A2350).** A slightly deeper, flatter set for staff density.
- **Chips.** Dynamic-pricing-rule chip #3D0F2A / #FFA3D1 (8.8:1). Offline chip #3D1418 / #FF9A9A (7.9:1).
- **HUD toast.** Solid #1E1606 with a #FFBE3D border and title (10.8:1) and #F5F2FF body (16.2:1).
- **Focus ring.** A double ring: inner 2px `{colors.focus-ring-inner}`, then outer 2px `{colors.focus-ring-outer}`, offset 2px from the control. One of the two rings holds ≥3:1 on any backing. Over a pure-white frame the magenta inner ring is 3.0:1; on any dark surface the white outer ring is ≥14:1. The double ring is what separates focus from the single magenta outline on `game-tile-featured`.
- **Scrim (`scrim-base` #0A0816).** Mandatory behind all text over video or art. It is verified against the **brightest** plausible frame (#FFFFFF), not the darkest. The hero copy (eyebrow, title, meta, CTAs) must stay inside the scrim's copy zone, the left 45% × bottom 50%. There the composite alpha is ≥0.80 and `text` measures ≥10.4:1 over a white frame. `text-muted` measures ≥4.6:1 there but is not used for hero copy; the eyebrow and meta line use `text`. Game-tile labels sit in the bottom 25% of the tile under `tile-scrim` (α 0.85): 12.2:1 over a white cover.

### Floor-tile states

These are the states the Hub must read at a glance. Contrast is measured on `hub-tile`.

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

State is always colour **plus** a text label. Device-group colour (PRD Floor view) never uses the tile body or state bar: it shows only as the dot in `group-chip`, in the tile corner, beside the group name.

### Device-group swatches

Owners pick each group's colour from a fixed set of six: `{colors.group-teal}` · `{colors.group-orange}` · `{colors.group-lime}` · `{colors.group-sand}` · `{colors.group-coral}` · `{colors.group-silver}`. Each measures ≥7.9:1 on `hub-tile` and ≥9.1:1 on black, so the same swatch serves HC. The dot is never the only cue; the group name always sits beside it. In the Owner app light theme the swatch picker draws each dot with a 1px `{colors.border-strong-light}` ring, because the light swatches fall below 3:1 on white.

### High-contrast theme (`*-hc` tokens)

- Pure black surfaces, white text and hairlines.
- Yellow accent #FFE14D on black, brightened semantics.
- The same state meanings, on the `state-*-hc` tokens (unassigned stays dashed).
- Inverted chips: rule = yellow with black text; offline = red with black text.
- Removes **all** art, video and translucency, and draws 2px white outlines on every interactive surface.
- The focus ring becomes a 4px yellow `{colors.focus-ring-hc}` ring (16.1:1). It is thicker than, and a different colour from, the always-on white outlines.

Every component token resolves to its `-hc` twin in HC.

### Light theme (`*-light` tokens, "Cool paper")

Source: [Light theme variants](mockups/color-themes-light.html), variant 1. Near-white with a faint indigo cast and deep indigo ink, so it reads as the same brand in daylight.

- **Where.** Owner app and all web surfaces (marketing, pricing, onboarding, partner portal). Kiosk, Hub, Kitchen display and Pi/QR screens never use it.
- **Mode rules.** The default follows the OS `prefers-color-scheme`. A manual `theme-toggle` (System · Light · Dark) in Owner app Account and in the web footer overrides it, remembered per device. HC (where it applies) overrides both.
- **Mapping.** Every component token resolves to its `-light` twin. Hub tones used by the Owner app map as: `hub-bg` and `hub-header` → `bg-light`, `hub-tile` → `surface-light`, `hub-line` → `line-light`. `metric-figure` uses `{colors.metric-figure-light}`.
- **Magenta splits in two.** `{colors.accent-light}` is for fills with white `{colors.on-accent-light}` ink (5.0:1). `{colors.accent-text-light}` is the darker magenta for links, the focus ring and any magenta ink; plain `accent-light` is never text.
- **Warning.** The amber fill `{colors.warning-light}` keeps dark `on-warning-light` ink but measures only 1.5:1 on `bg-light`, so every warning fill gets a 1px `{colors.warning-text-light}` edge, and warning ink and borders (variance over threshold, outstanding credit) use `warning-text-light`.
- **Focus.** A single 2px `{colors.focus-ring-light}` ring, offset 2px (≥5.9:1 on every light ground). There is no featured outline on light surfaces to confuse it with.
- **Media sections stay dark.** Full-bleed media on the marketing site (hero, media bands) keeps the dark tokens and `scrim` in both modes, because the scrim is verified only for light ink.
- **Verified pairs.** All load-bearing pairs were computed against `bg-light`, `surface-light` and `surface-2-light`: text ≥15.0, muted ≥6.3, accent-text ≥5.9, link on chip backgrounds ≥5.8, semantic and state ink ≥4.5 (lowest: `ok-light` and `state-free-light` 4.5 on `surface-2-light`), `border-strong-light` ≥3.7, focus ring ≥5.9. **No light token needed adjusting.** Ratios sit beside the tokens in the frontmatter.

Avoid: gradients as decoration (the scrims are the only gradients), translucent panels, brand colours chosen by a branch, a second accent.

## Typography

**Sora** (SIL OFL) is bundled as a self-hosted web font in the shared web UI + design-token codebase that every surface renders from. Kiosk and Hub run that UI inside Tauri (WebView2 on Windows; Tauri 2 mobile on Android). Rendering is therefore identical everywhere and never depends on a system font. **Tabular numerals are mandatory** for every timer, countdown and currency amount (`hud-numeric`, `hub-numeric`, and any numeric cell). Currency appears as ₹ in examples and is locale-formatted in the product.

Copy is English only. Strings stay externalised so other languages can return later. Do not fit layouts to English string lengths so tightly that +30% breaks them.

### Kiosk ramp

Authored for 1920×1080; also used on Pi TV screens.

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

### Hub ramp

Also used by the Kitchen display; the seat number uses `hub-numeric`.

| Role | Size / weight | Use |
|---|---|---|
| `hub-numeric` | 26/800 | Floor-tile time left, totals |
| `hub-title` | 18/600 | Titles |
| `hub-body` | 15/400 | Body text |
| `hub-label` | 13/500 | Secondary tile lines |
| `hub-micro` | 11/700 | Uppercase; state tags and rail labels |

Nothing in a dense staff view goes below 11px.

### Web

- `web-display` 40/300 and `web-body` 16/400 serve the Owner app (responsive web/PWA), onboarding, partner portal and marketing. Owner figures use `hub-numeric`.
- `{typography.web-hero}` 64/200 is the marketing and onboarding hero title at ≥1024px wide ([marketing home mock](mockups/key-web-marketing-home.html)). Below 1024px the hero drops to `web-display`.

## Layout & Spacing

The spacing scale is 4-based: 4 · 8 · 12 · 16 · 24 · 32 · 40 · 56 · 64 · 80 · 96 · 128 (`{spacing.1}` … `{spacing.12}`). Every gap, padding and gutter on every surface comes from this scale; there are no off-scale values. `{spacing.touch-min}` (44px) is the floor for every interactive target on every surface.

**Spacing posture: airy and premium on Hub, Owner app, Kitchen display and Web.** These surfaces read like a console dashboard, not a back-office grid: generous page gutters, wide section gaps, and roomy panel and card padding, so each region reads as one calm block under rush pressure. Density comes from fewer, clearer things on screen, never from tighter spacing. The Kiosk keeps its own spacing (below). The accepted cost: the Hub floor shows fewer tiles per screen and, at high device counts, scrolls and lets staff collapse zones (behaviour in EXPERIENCE.md State Patterns and Responsive & Platform).

| Surface | Page gutter | Section gap | Panel / card padding | Header height |
|---|---|---|---|---|
| Hub (desktop, tablet) | `{spacing.hub-gutter}` 32 | `{spacing.hub-section-gap}` 40 | `{spacing.hub-panel-padding}` 24 (tiles `{spacing.hub-tile-padding}` 16) | `{spacing.hub-header-height}` 72 |
| Hub (phone) | `{spacing.hub-gutter-phone}` 16 | `{spacing.6}` 32 | `{spacing.4}` 16 | `{spacing.9}` 64 |
| Owner app | `{spacing.owner-gutter}` 40 (phone `{spacing.owner-gutter-phone}` 16) | `{spacing.owner-section-gap}` 56 | `{spacing.owner-card-padding}` 32 (phone `{spacing.owner-card-padding-phone}` 24) | `{spacing.owner-header-height}` 64 |
| Kitchen display | `{spacing.kitchen-gutter}` 24 | `{spacing.kitchen-column-gap}` 24 between columns | `{spacing.kitchen-ticket-padding}` 16 | `{spacing.kitchen-header-height}` 64 |
| Web: marketing | `{spacing.web-gutter}` 56 (phone `{spacing.web-gutter-phone}` 16) | `{spacing.web-section-gap}` 128 (phone `{spacing.web-section-gap-phone}` 64) | `{spacing.web-card-padding}` 32 (phone `{spacing.web-card-padding-phone}` 24) | `{spacing.web-header-height}` 80 (phone `{spacing.web-header-height-phone}` 64) |
| Web: onboarding, partner portal | same as marketing | `{spacing.web-app-section-gap}` 56 | `{spacing.web-card-padding}` 32 | `{spacing.web-header-height}` 80 |

Inside a panel, related rows sit `{spacing.3}`–`{spacing.4}` apart and unrelated groups `{spacing.5}`–`{spacing.6}` apart. Buttons that sit side by side keep ≥`{spacing.2}`; destructive actions keep the wider separation set per component (for example, `tile-sheet`).

### Kiosk

Landscape, responsive. Designed at 1920×1080; the layout **reflows** (it never scales a 1080p canvas) at four breakpoints:

| Breakpoint | Featured row | Games / Shop grid | Hero copy |
|---|---|---|---|
| 1366 | 4 tiles + All games | 5 columns | ≤720px wide |
| 1920 | 6 tiles + All games | 7 columns | ≤864px wide |
| 2560 | 8 tiles + All games | 9 columns | ≤864px wide |
| Ultrawide (21:9) | 8 tiles + All games | 9 columns, content max 2560px centred | anchored to the left 45% of the media frame |

The type ramp and spacing tokens are unchanged across breakpoints; Pi TV screens use the 1920 layout.

- `{spacing.kiosk-margin}` (56px) outer margin.
- The top bar sits 32px from the top and is 64px tall:
  - Left: `logo-slot`.
  - Centre: `pill-nav`.
  - Right cluster, left to right: `reconnecting-indicator` (only when shown), `volume-control`, `messages-bell`, `call-staff-button`, `time-chip`, account chip.
- Home ([Home mock](mockups/key-kiosk-home.html)):
  - The featured `game-tile` row sits upper-left and ends with an `all-games-tile`.
  - The hero copy block is bottom-left and ≤864px wide (inside the scrim copy zone).
  - `video-control` sits at the bottom-right of the hero.
- Games, Apps and Shop: left filter rail, search, then the grid, following the ggLeap structure ([imports/ggleap-04-games-library.png](imports/ggleap-04-games-library.png), [imports/ggleap-06-shop.png](imports/ggleap-06-shop.png)).
- Games also carries a `launcher-card` row and a `most-played-carousel` above the grid ([Games mock](mockups/key-kiosk-games.html)).
- The search field sits right-aligned in the content title row on Games and Shop.
- Shop opens `cart` as a right-side panel ([Shop & cart mock](mockups/key-kiosk-shop.html)).
- The Account page puts `account-subnav` on the left and content on the right ([imports/ggleap-07-account.png](imports/ggleap-07-account.png); [Account mock](mockups/key-kiosk-account.html)).
- Overlay HUD: `hud-pill` and `hud-toast` at the top-right over the running game; the grace `lock-screen` centres its countdown with Extend beside it ([HUD & time-up mock](mockups/key-kiosk-hud-lock.html)).
- Login: a single panel over full-bleed media ([imports/ggleap-01-login.png](imports/ggleap-01-login.png); [Login mock](mockups/key-kiosk-login.html)), with `video-control` bottom-right.

### Hub

Windows desktop, Android tablet and phone.

- Desktop and tablet:
  - A persistent left icon rail (`{spacing.hub-rail-width}`), a header (`{spacing.hub-header-height}`), and a main area with `{spacing.hub-gutter}` padding.
  - Floor view groups tiles by zone ([Floor view mock](mockups/key-hub-floor.html); six columns at 1920 wide with the rail and gutters). Zones sit `{spacing.hub-section-gap}` apart; inside a zone the tile-grid gap is `{spacing.floor-grid-gap}` (24px); tiles are ≥128px tall with `{spacing.hub-tile-padding}` inside. Each zone has a header row (zone name, tile count, collapse control) `{spacing.4}` above its grid.
  - The zone header also carries a muted line with the group's hardware and rate ("RTX 4070 · 240 Hz · ₹100/hr"), and Unassigned devices gather in their own "Unassigned" section at the end of the floor.
  - Each tile carries its `group-chip` in the bottom-right corner, above the state bar.
  - POS is a single screen: `pos-product-grid` on the left, `cart` on the right, `settle-bar` along the bottom; the grid and cart sit `{spacing.hub-section-gap}` apart ([POS mock](mockups/key-hub-pos.html)). `payment-split` sits in the cart below the totals.
  - Sheets (`tile-sheet`) and cards use `{spacing.hub-panel-padding}` ([Tile sheet mock](mockups/key-hub-tile-sheet.html)).
  - `tile-sheet` docks to the right of the floor and the floor grid reflows (four columns at 1920) while it is open.
  - Shift close: the drawer count (`denomination-counter`) and the cash check (`variance-readout`, then `inline-pin` + reason when over threshold) as two panels `{spacing.hub-section-gap}` apart, each padded `{spacing.hub-panel-padding}` ([Shift close mock](mockups/key-hub-shift-close.html)).
- Phone (below **840px** wide: phones and small portrait tablets; ≥840px keeps the rail):
  - Same tile grid and same screens, but the rail becomes `hub-bottom-bar` (Floor · POS · Players · Shift · More) so the floor fits two tile columns ([Hub phone mock](mockups/key-hub-phone.html)).
  - The header drops backup status and the clock, and `chip-rule` / `chip-offline` move to a row under the header.
  - Gutter drops to `{spacing.hub-gutter-phone}` and the tile gap to `{spacing.floor-grid-gap-phone}` (12px); touch targets stay ≥44px.
  - POS: a full-width `pos-product-grid` with `phone-cart-bar` pinned above `hub-bottom-bar` ("3 items · ₹310 · View"). Tapping it opens `cart` and `settle-bar` as a full-height sheet.

### Kitchen display

Tablet landscape, four status columns (New → Preparing → Ready → Served) `{spacing.kitchen-column-gap}` apart, with tickets stacked newest-first `{spacing.kitchen-ticket-gap}` apart ([Kitchen board mock](mockups/key-kitchen-board.html)). Each column header shows its ticket count. `kitchen-top-bar` runs across the top, left to right: branch name · clock · open tickets with the oldest age ("5 open · oldest 9 min") · LAN status (Connected / Reconnecting) · mute-chime toggle.

### Owner app

- One responsive layout from phone to desktop.
- Desktop: `owner-nav` as a left sidebar (`{spacing.owner-sidebar-width}`) and content up to about 1200px wide with `{spacing.owner-gutter}`.
- Phone: `owner-nav` as a bottom tab bar (64px) and a single column with `{spacing.owner-gutter-phone}`.
- Today stacks one branch card per branch, `{spacing.owner-section-gap}` between sections ([Owner Today mock](mockups/key-owner-today.html), phone and desktop; [Price change mock](mockups/key-owner-price-change.html), phone).
- On desktop, figures inside a branch card sit in a two-column grid.
- Phone: each Today branch card ends with a **Live view** button (shown with `tier-lock` below Premium).

### Web surfaces

- Onboarding and the partner portal are staff mood, content up to about 1200px wide.
- Onboarding puts `wizard-progress` in a left column beside the step content ([Onboarding install-Hub mock](mockups/key-web-onboarding-install-hub.html)). Below 840px it becomes a row of 10 horizontal dots plus the current step name above the content.
- Marketing is player mood, with full-bleed media sections separated by `{spacing.web-section-gap}` ([Marketing home mock](mockups/key-web-marketing-home.html), 1440 and 390).
- Pricing puts its Cafés and Partners panels under a `tab-control` ([Pricing mock](mockups/key-web-pricing.html)); the Cafés panel has a second `tab-control` for Monthly · Yearly above three equal plan cards (no emphasised plan) and one primary "Get started" above the table.
- Phone navigation: a hamburger opens a full-screen nav; the "Get started" button stays visible in the header.
- The partner portal uses the staff-mood card and table composition shown in the [Partner portal mock](mockups/key-web-partner-portal.html).
- Page contents and order for these surfaces live in EXPERIENCE.md Information Architecture.

### Thermal print

Receipts, GST invoices, credit notes, KOT and Z-report follow the structure in EXPERIENCE.md. Paper width is a branch setting: **58mm or 80mm**, and the layout reflows by width.

- **Font.** The printer's built-in ESC/POS font (Font A), never rasterised Sora. Body is normal size; headings and the total use bold + double-height.
- **Columns.** 80mm: 48 characters per line; item lines show name, qty, rate and amount in columns. 58mm: 32 characters; the name takes its own line and qty × rate and amount go on the line below.
- **Header.** Text only, no logo image: branch name in bold double-width + double-height, then address, GSTIN and phone in normal size, centred.
- **Amounts** are right-aligned; rules are a row of `-` characters; KOT seat numbers print double-width + double-height.

### Pi / QR screens

- TV screens use the 1920 Kiosk layout and type ramp. `tv-idle` puts `logo-slot` top-left, "TV n" and console rates bottom-left inside the scrim copy zone, and the QR block bottom-right.
- `qr-start-page` and `live-timer` are a single portrait column ([Pi console mock](mockups/key-pi-console.html)) on the player's phone with `{spacing.web-gutter-phone}` gutters.

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

### Shared

- **focus-ring.** A double ring (magenta inner, white outer, offset 2px); see it beside the single featured outline in the [Home mock](mockups/key-kiosk-home.html). In HC it is a 4px yellow ring. When and where it shows: EXPERIENCE.md Component Patterns.
- **text-field.** `surface-2` fill and a `border-strong` 1px border, `rounded.sm`, ≥44px tall, with the label above.
  - Focus: a 2px accent border plus `focus-ring`.
  - Error: a 2px danger border, a ⚠ icon and the message below in `danger`. Colour is never the only error marker.
  - Password variant: a trailing show/hide toggle.
- **button-primary / button-secondary / button-warning.**
  - Primary: a magenta pill in `kiosk-cta`, ≥340px wide on the Kiosk hero, one per view.
  - Secondary: a `surface-2` pill with a `border-strong` border.
  - Warning: an amber pill, used for Extend in `hud-toast` and `extend-options`.
- **interaction-states.** One rule for every button, tile, card and row:
  - Hover: the surface tints 6% lighter; accent fills 8% lighter. In the light theme, hover tints 6% darker, since white cannot lighten.
  - Pressed: 6% darker than rest, plus a 1px inset.
  - Disabled: 40% opacity, no hover, a not-allowed cursor, and the reason in text nearby at full ink (disabled controls are exempt from contrast; the reason is not).
  - HC: hover thickens the 2px white outline to 3px and pressed insets it; no tints. The focus ring is always drawn separately from these states.
- **theme-toggle.** A `tab-control` with System · Light · Dark, in Owner app Account and the web footer. Owner app and web only.
- **undo-toast.** A `surface-2` bar with a `border-strong` edge. It shows what happened (with the amount, tabular) and an **Undo** action in accent. Hub: bottom-centre, above `settle-bar` or `hub-bottom-bar`. Kitchen display: bottom of the screen ([Tile sheet mock](mockups/key-hub-tile-sheet.html), [Kitchen board mock](mockups/key-kitchen-board.html)). It shows the seconds remaining ("8 s") before Undo.
- **alert-badge.** A red pill with `on-danger` ink: a count in the Hub header, or a short tag ("NEW", "CALL", "TIME UP") on a tile or ticket. It is the persistent visual twin of every alert sound.
- **status-badge.**
  - A small uppercase tag in `hub-micro`: state or semantic colour as text and a 1px border, on `hub-tile` (ratios as in Colors).
  - Also used for command status in the Owner app: queued on this device = text-muted, pending = info, applied = ok, rejected = danger, expired = text-muted.
- **tier-lock.** The gated control is shown with a lock glyph in `text-muted`, plus "Available on Basic" (or Premium) and an underlined "Upgrade" link. Staff and owner surfaces only.

### Player (Kiosk, HUD)

- **scrim / tile-scrim.**
  - The hero scrim is the two-gradient overlay in the frontmatter; all hero copy stays in its copy zone.
  - `tile-scrim` holds α 0.85 over the bottom 25% of each cover and fades out by 55%.
  - Both are omitted only when media is absent (solid fallback) or in HC.
- **video-control.** ([Home mock](mockups/key-kiosk-home.html), [Login mock](mockups/key-kiosk-login.html)) A 44px round pause/play button (solid `bg`, `border-strong` ring, `text` icon) at the bottom-right of the login and Home hero. Its icon shows the action it will take.
- **login-panel.**
  - A solid `surface` panel, `rounded.xl`, centred over full-bleed muted video.
  - Contents: username and password `text-field`s (password with show/hide), the primary button, an underlined "Forgot password?" link and an underlined "Create account" link.
  - Footer: PC name and version.
  - No language picker, no social login.
  - Mocked at rest, after a failed submit, and with the Hub unreachable in the [Login mock](mockups/key-kiosk-login.html).
- **signup-form.** (Spine-only; no mock.) The same panel shell as `login-panel`. Fields are username, password, phone and name. Below them sits the required consent checkbox with its consent text (a `border-strong` box, ≥44px hit area; underlined Terms and Privacy Policy links), then the primary button.
- **pill-nav / pill-nav-item-active.** Home · Games · Apps · Shop in a `surface` pill track. The active item inverts to a `text` fill with `bg` ink. The Shop item is absent on Free. Top-bar cluster (`pill-nav`, `volume-control`, `messages-bell`, `call-staff-button`, `time-chip`, account chip) as in the [Home mock](mockups/key-kiosk-home.html). On the Account page the tab the player came from stays active ([Account mock](mockups/key-kiosk-account.html)).
- **time-chip.** An always-visible pill at the top-right showing a status dot and time left in `hud-numeric`. At ≤10 min the chip gains a 2px `warning` border and a ⚠ glyph replaces the dot. The change is never colour alone.
- **reconnecting-indicator.** A quiet `surface` pill with `text-muted` "Reconnecting…" and a small static icon. It appears only while the Hub is unreachable.
- **volume-control.** A speaker icon button. It opens a horizontal slider: `border-strong` track, `text` fill and thumb, a 44px thumb hit area.
- **messages-bell.** A bell icon button. Unread messages add an `info` dot plus a count, and the accessible name carries the count.
- **call-staff-button.** A "?" icon with the visible label "Call staff", in a `surface` pill with a `border-strong` edge. After a tap it shows "Staff notified".
- **account-menu.** A `surface` panel containing: balance, Extend, Messages, Account, PC settings, Lock, Log out ([Account mock](mockups/key-kiosk-account.html)). Its header shows the player's name, wallet and time left, and the Extend row shows the Ctrl+Shift+E key caps. PC settings opens a `card` panel: mouse sensitivity (slider), display resolution and refresh (allowed list), audio output device and keyboard layout.
- **account-subnav.** A vertical list: Details · Wallet & packs · Order history · Change password. The active item is `text` with a 3px left bar; the others are `text-muted`.
- **hero.** ([Home mock](mockups/key-kiosk-home.html)) Game video (muted) → cover → `bg`, under the scrim. Contents: eyebrow in `text`, the `kiosk-display` title, a meta line in `text`, Play now (primary) and **Game details** (`button-secondary`), which opens a `card` panel with genre, age rating and launcher.
- **game-tile / game-tile-featured.** ([Home mock](mockups/key-kiosk-home.html), [Games mock](mockups/key-kiosk-games.html))
  - A portrait cover-art tile, 140×196. The featured tile is 190×260 with a 3px magenta outline, offset 3px.
  - The name sits bottom-left in `kiosk-tile-name` over `tile-scrim`.
  - The fallback is `surface` with the name centred.
- **all-games-tile.** The last tile of the featured row: `surface` with a `border-strong` edge, a grid icon and "All games".
- **most-played-carousel.** ([Games mock](mockups/key-kiosk-games.html)) A horizontal row of `game-tile`s titled "Most played here". Each tile carries a rank badge ("#1", "#2", …) at the top-left, with `bg` fill and `text` ink.
- **launcher-card.** ([Games mock](mockups/key-kiosk-games.html)) A `surface` card with the launcher's logo (Steam, Epic, Riot), its name, and the line "Use your own account".
- **filter-rail.** A left column of categories and tags. The active item is in `text`, the others in `text-muted`. A search `text-field` sits at the top of the content area.
- **shop-item-card.** ([Shop & cart mock](mockups/key-kiosk-shop.html))
  - Contents: photo, name, price (tabular) and an Add button.
  - `popular-badge` sits at the top-left of the photo when applicable.
  - Out of stock: the photo dims; the name and the "Out of stock" tag stay at full ink.
- **popular-badge.** An uppercase "POPULAR" pill: `bg` fill, `text` ink, `border-strong` edge.
- **cart.**
  - Kiosk: a right-side `surface` panel, `rounded.xl`. It lists item lines with ±1 steppers, bundle components indented, and the total in `hud-numeric`, with the primary "Place order" button at the bottom ([Shop & cart mock](mockups/key-kiosk-shop.html)). Above the button, the payment choice: Add to session bill · Pay now from wallet.
  - Hub POS: the same anatomy on `hub-tile`, with the total in `hub-numeric` ([POS mock](mockups/key-hub-pos.html)). The cart header names the attached player and seat with a "Change" link. On phone it opens from `phone-cart-bar` as a full-height sheet.
- **card / plan-picker.**
  - `card` is the generic `surface` container.
  - `plan-picker` lays out plan options as cards. The selected card gets a 2px `text` border. Expired packs show muted ink and an "Expired" tag and cannot be selected (the expired treatment is shown on the Account page in the [Account mock](mockups/key-kiosk-account.html); the plan picker itself is spine-only). An expired pack also shows its unused time ("0h 45m unused").
- **hud-pill.** ([HUD & time-up mock](mockups/key-kiosk-hud-lock.html)) A small always-on-top pill showing time left or elapsed, and balance. Solid `bg`, never translucent.
- **hud-toast.**
  - A solid amber-bordered panel (~640px) at the top-right.
  - Contents: a title in warning amber, a body in `hud-toast-text`, and actions (Extend = `button-warning`, Dismiss = `button-secondary`).
  - It shows the Extend hotkey as key caps: **Ctrl+Shift+E**.
- **extend-options.** The expanded-HUD panel in the `hud-toast` palette. It lists time options with live prices as `button-secondary` rows, plus the payment source (wallet, pack or credit).
- **lock-screen.** Variants (only the grace countdown is mocked, in the [HUD & time-up mock](mockups/key-kiosk-hud-lock.html)):
  - Grace countdown: `kiosk-display` digits over the still-visible game behind the scrim, with an **Extend** `button-warning` beside the countdown.
  - Fullscreen-exclusive games: no Kiosk visual is drawn over the game. At 5 and 1 min the Kiosk flashes the taskbar button and plays the warning sound (known limitation: see EXPERIENCE.md Accessibility Floor).
  - Pay at counter or Time's up: a solid `bg` lock with the "Pay at counter" or "Time's up" message.
  - Staff handles: a solid `bg` lock held over the paused game for staff, when the branch uses the "staff handles" time-up mode.
  - Forced end: the forced-end message.
- **logo-slot.** The branch logo in the Kiosk top-left and on Pi screens. It is the only branch-specific visual.

### Player (Pi / QR)

Player mood, dark only (HC applies to the TV screens). All five are shown in the [Pi console mock](mockups/key-pi-console.html). Layout: see Layout & Spacing, Pi / QR screens.

- **tv-idle.** ([Pi console mock](mockups/key-pi-console.html), frame 1) Muted game video from the same media pipeline as the Kiosk (video → cover → `bg`) under `scrim`. Branch `logo-slot`, "TV 3" in `kiosk-headline`, the console rates (tabular), and a large QR (≥360px, white quiet zone, `rounded.md` frame) labelled "Scan to start". No ads in Phase 1.
- **tv-warning.** ([Pi console mock](mockups/key-pi-console.html), frame 4) Shown for 5 s when the Pi switches the TV to its input at 5 and 1 min: solid `bg` with an 8px `warning` frame, the minutes left in `kiosk-display`, and the line "ask at the counter to extend" in `kiosk-headline`. Then the TV switches back to the console.
- **tv-lock.** ([Pi console mock](mockups/key-pi-console.html), frame 5) **No grace.** At time-up the TV locks immediately: solid `bg`, `logo-slot`, and "Time's up — see the counter." in `kiosk-headline`, nothing else. Held until staff end the session; staff handle it at the counter. Powering the TV down is per branch (HDMI-CEC standby or a smart plug).
- **qr-start-page.** ([Pi console mock](mockups/key-pi-console.html), frame 2) The phone page the QR opens. Header: `logo-slot` and "TV 3". Body: the console plans as `plan-picker` cards with tabular prices, and one primary "Request" button. After the request: "Waiting for the counter — pay at the counter to start."
- **live-timer.** ([Pi console mock](mockups/key-pi-console.html), frame 3) After the start, the phone shows one `surface` card: "Time left on TV 3", the time in tabular 96px Sora 200, and "Extend at the counter" in `kiosk-body`. No actions.

### Staff (Hub, Kitchen)

- **hub-rail / hub-bottom-bar.** The left icon rail (`{spacing.hub-rail-width}`) has `hub-micro` labels; the active item is filled `hub-line`. On phone it becomes a 64px bottom bar with five labelled items, Floor · POS · Players · Shift · More, and a `border-strong` top edge. More opens the remaining destinations ([Hub phone mock](mockups/key-hub-phone.html)) as a bottom sheet listing Sessions (first), Inventory, Reports, Settings and System as rows. The bottom bar replaces the rail below 840px wide.
- **hub-header.** ([Floor view mock](mockups/key-hub-floor.html)) Contents, in order:
  - Branch name.
  - The active **chip-rule** (dynamic pricing rule).
  - Connectivity: **chip-offline** when internet or cloud is unreachable, with text naming which.
  - On paid tiers, cloud backup status in `hub-label` muted: last upload, and pending batches when non-zero.
  - The `alert-badge` count.
  - Clock.
  - Signed-in staff.
- **floor-tile.** ([Floor view mock](mockups/key-hub-floor.html): all eight states, the console variant and keyboard focus)
  - Body: `hub-tile` with a `border-strong` 1px edge.
  - Top row: device name plus a `status-badge`.
  - Middle: time left or elapsed in `hub-numeric`.
  - Bottom: player name plus a secondary line in `hub-label`.
  - A 3px state-colour bar runs along the bottom edge.
  - Paused: `state-paused` bar and badge, a pause icon, and the timer frozen. The secondary line shows the pause reason (Player stepped away · Hardware issue · Power cut · Other).
  - Unassigned: a 2px dashed `state-unassigned` border with no state bar and no `group-chip`.
  - `group-chip` sits in the bottom-right corner above the state bar.
  - `tile-order-badge` and `alert-badge` sit at the top-right corner. They are not separate tap targets; tapping the tile opens the sheet at the relevant section.
- **floor-tile-console.** ([Pi console mock](mockups/key-pi-console.html), frame 6: controller check) A `floor-tile` with a controller glyph before the device name and "TV n" in the secondary line. Its states and timer are identical to a PC tile. After time-up it carries a "CHECK CONTROLLERS" `status-badge` in `warning` until staff mark Returned or Missing in the sheet. A QR start request shows an `alert-badge` "REQUEST".
- **group-chip.** A small neutral `surface-2` pill with a `hub-line` edge: an 8px dot in the group's swatch plus the group name in `hub-micro` muted. It never fills the tile, so state colour stays dominant.
- **tile-order-badge.** A small amber tag in `hub-micro` with `hub-tile` ink (11.1:1). It reads "ORDER" or "READY".
- **tile-sheet.** ([Tile sheet mock](mockups/key-hub-tile-sheet.html)) A `surface` sheet with the tile's actions: start · extend · order · transfer · pause/resume · lock · end. It also shows the rate and `bill-breakdown`. End and Lock are `button-secondary` and sit apart from Extend and Order, ≥`{spacing.5}` (24px) between the two groups.
- **inline-pin.** ([Tile sheet mock](mockups/key-hub-tile-sheet.html), [Shift close mock](mockups/key-hub-shift-close.html)) A four-to-six-cell PIN field rendered inside the current sheet or row. Cells are `surface-2` with a `border-strong` edge; a filled cell shows a `text` dot. Radius is `rounded.sm`.
- **player-lookup.** One phone `text-field` with the results directly below it and a "Guest" `button-secondary` beside it. A result row shows the name and phone. Any credit outstanding is shown first, in `warning` text.
- **pos-product-grid.** ([POS mock](mockups/key-hub-pos.html))
  - A category strip above a grid of product tiles (`hub-tile`, `border-strong`, `rounded.md`). Each tile shows a photo, name and tabular price.
  - Favourites carry a star and sort first.
  - One "Search or scan barcode" field sits above the grid.
  - Phone: full width, with `phone-cart-bar` below it.
- **phone-cart-bar.** (Hub phone POS.) A 64px bar pinned above `hub-bottom-bar` on `hub-header` with a `border-strong` top edge: "3 items · ₹310 · View", tabular. Tapping it opens `cart` + `settle-bar` as a full-height sheet.
  - Out of stock: the photo dims and the "Out of stock" tag stays readable.
- **settle-bar.** ([POS mock](mockups/key-hub-pos.html)) Pinned to the bottom on `hub-header` with a `border-strong` top edge. It shows the total in `hub-numeric`, the payment-method buttons (cash · UPI · card · wallet · credit, with no QR), and Settle (`button-primary`).
- **payment-split.**
  - One row per method used: method label, amount field, and a reference field where needed (UPI: last 4 digits; card: terminal reference).
  - The running remainder is in `hub-numeric`: `warning` while non-zero, `ok` at zero, always with the words "Remaining" or "Paid in full".
- **bill-breakdown.** ([Tile sheet mock](mockups/key-hub-tile-sheet.html), [POS mock](mockups/key-hub-pos.html)) Line items, left-aligned labels with right-aligned tabular amounts, then a ruled total in `hub-numeric`.
- **denomination-counter.** ([Shift close mock](mockups/key-hub-shift-close.html)) One row per note or coin: denomination, a count field, and a tabular subtotal. The running total sits at the foot. A "Single total" toggle swaps the rows for one field.
- **variance-readout.** ([Shift close mock](mockups/key-hub-shift-close.html))
  - Counted, Expected and Difference as three `hub-numeric` figures, `{spacing.6}` apart. Difference = counted − expected and carries a sign and the word "over" (+, extra cash) or "short" (−).
  - Over threshold: a `warning` border and the label "Over threshold", never red.
  - A "Breakdown" underlined link sits below.
- **chip-rule / chip-offline.** Header pills, per Colors.
- **kitchen-top-bar.** A 64px `hub-header` bar: branch name in `hub-title`, clock, "5 open · oldest 9 min" (tabular), a LAN `status-badge` ("CONNECTED" in `ok`, "RECONNECTING" in `warning`), and a 44px mute-chime toggle. While muted, the toggle shows a struck-through bell and the "Chime off" label; new tickets still carry their `alert-badge`.
- **kitchen-ticket.** ([Kitchen board mock](mockups/key-kitchen-board.html))
  - A `hub-tile` card with a `border-strong` edge.
  - Contents: the seat number in `hub-numeric`, items and modifiers, order id, and elapsed time (tabular).
  - A new ticket carries an `alert-badge` "NEW" until it is advanced.
  - Ready tickets gain a `state-free` edge plus a "READY" tag.
  - A NEW ticket also carries a `danger` edge until advanced, and Served tickets drop to `text-muted` ink.
  - Cancelled (from the Hub only): ink drops to `text-muted`, a "CANCELLED" `status-badge` replaces the stage tag, then the ticket leaves the board. The ticket has no cancel control.
- **ad-banner.** A plain `surface` strip with an "Ad" label at the bottom of the Free-tier Android Hub. It never overlaps the floor grid, cart or settle bar and never shares an edge with `settle-bar` or `cart`. It never appears on the Kiosk in Phase 1.
- **ad-interstitial.** (Free-tier Android Hub, shown only on return after the idle lock.) A full-screen ad on solid `bg` with an "Ad" label and a `button-secondary` Skip at the top-right that counts down "Skip in 5 s" and then reads "Skip ad".

### Owner and Web

- **metric-figure.** An Owner app figure: `hub-label` muted label, `hub-numeric` value, and data age in `hub-micro` muted below ("as of 2 min ago"; up to 15 min on Basic, 5 min on Premium, or "Live" via Tunnel). The Audit report's "Preparing audit…" state is `hub-label` muted text over a `surface` panel in place of results, no spinner overlay. On Today branch cards, the per-shift cash variance (counted − expected, signed, "over"/"short") sits beside cash in drawer; over threshold it is flagged with a `warning` edge and label ([Owner Today mock](mockups/key-owner-today.html)). An offline branch keeps its last figures with ink dropped to `text-muted`, a "Branch offline" `status-badge`, and growing data age.
- **owner-nav.** ([Owner Today mock](mockups/key-owner-today.html)) Owner app navigation: Today · Reports · Alerts · Manage · Account. Desktop: a left sidebar (`{spacing.owner-sidebar-width}`) on `hub-bg` with icon + `hub-label` text; the active item is filled `hub-line`. Phone: a 64px bottom tab bar with icon + `hub-micro` label and a `border-strong` top edge. Live view sits in the sidebar on desktop and as a button on each Today branch card on phone (locked below Premium with `tier-lock`). Follows the light theme when active.
- **command-row.** ([Price change mock](mockups/key-owner-price-change.html): all four states) An Owner app command line: what changed, the scope (branch or "All branches"), `status-badge`, time queued, and Retry or Resend when applicable.
- **wizard-progress.** ([Onboarding install-Hub mock](mockups/key-web-onboarding-install-hub.html)) The onboarding step list in a left column (~256px): "Step n of 10" in `hub-micro`, then one ≥44px row per step with a round tabular step number. Done steps: `text` ink and an `ok` check; the current step: `hub-line` fill and a 2px `text` ring on its number; later steps: `text-muted`. Skippable steps carry a muted "Can skip" tag. Below 840px: a row of 10 dots (done = `ok` fill, current = `text` fill with a 2px ring, later = `border-strong` ring) and "Step 3 of 10 · Install Hub" in `hub-label`.
- **tab-control.** ([Pricing mock](mockups/key-web-pricing.html)) A segmented pill switch for peer views on one page (marketing Pricing: Cafés · Partners and Monthly · Yearly; Owner app Subscription billing cycle; `theme-toggle`). Same anatomy as `pill-nav`: a `surface` track with `{spacing.1}` inset, items ≥44px tall, the selected item inverted to `text` fill with `bg` ink. Not used for site or app navigation.
- **pairing-code.** ([Onboarding install-Hub mock](mockups/key-web-onboarding-install-hub.html): waiting and expired states) The onboarding code display: an 8-character code in `web-display` with tabular figures, grouped 4 + 4, with a "Copy code" `button-secondary` beside it and a countdown. No QR. It sits in a `surface` card with a `border-strong` edge.

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
| Light theme only on Owner app and web; `accent-text-light` for magenta ink | Light theme on Kiosk, Hub, Kitchen or Pi; `accent-light` as text |
| Group colour as a dot in `group-chip` | Group colour as a tile fill or state bar |
| State = colour + label on every floor tile | Colour-only state |
| Tabular numerals for timers and currency | Proportional digits that jitter while counting |
| Branch logo in the logo slot | Branch colours, background art, or fonts |
| Subtle, short transitions; none in high-contrast | Flashy motion, parallax, autoplaying sound |
| Staff mood: no art, no video, denser | Cinematic media on Hub, Kitchen, Owner or Partner surfaces |
| 44px minimum targets on every surface | Shrinking Hub phone targets to fit more tiles |
| On-scale spacing tokens; airy gutters and padding on Hub, Owner, Kitchen and Web | Off-scale gaps (10px, 20px) or tightening spacing to fit more tiles |
