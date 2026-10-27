/**
 * The viewport the Hub UI is in, shared by every component that must behave differently on a
 * phone: below 840px the rail becomes the bottom bar, the header is the phone header and a tile
 * opens as a full-height sheet. The same query drives the CSS breakpoints.
 */
export const PHONE_QUERY = "(max-width: 839.98px)";

export const viewport = $state({
  /** Narrower than 840px (a phone, or a tablet in a narrow split screen). */
  phone: false,
  /** A full-height tile sheet covers the screen: the header and bottom bar are inert. */
  sheetFull: false,
});

if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
  const query = window.matchMedia(PHONE_QUERY);
  viewport.phone = query.matches;
  query.addEventListener("change", (e) => (viewport.phone = e.matches));
}
