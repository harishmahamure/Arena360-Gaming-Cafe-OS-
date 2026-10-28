// Receipt and notification settings (Story 5.15): the keys and the text preview. The preview
// wraps exactly as the printer will: at word boundaries, a word longer than the line is cut.

export const KEY_FOOTER = "receipt.footer";
export const KEY_ADDRESS = "receipt.address";
export const KEY_PHONE = "receipt.phone";
export const KEY_DEFAULT = "receipt.default";
export const KEY_BRANCH_NAME = "branch.name";
export const KEY_TIER = "licence.tier";
export const RECEIPT_KEYS = [KEY_FOOTER, KEY_ADDRESS, KEY_PHONE, KEY_DEFAULT];
export const RECEIPT_CHOICES = ["print", "none"] as const;
export const FOOTER_MAX_LINES = 3;
export const ADDRESS_MAX_LINES = 3;
export const LINE_MAX_CHARS = 96;

export const NOTIFY_MESSAGES = [
  { key: "notify.whatsapp_receipt", id: "whatsapp_receipt" },
  { key: "notify.credit_reminder", id: "credit_reminder" },
  { key: "notify.low_balance", id: "low_balance" },
  { key: "notify.session_ending", id: "session_ending" },
] as const;
export const NOTIFY_KEYS: string[] = NOTIFY_MESSAGES.map((m) => m.key);

/** Characters per line of a paper width: Font A fits 32 on 58 mm and 48 on 80 mm. */
export const lineWidth = (paperMm: string) => (paperMm === "58" ? 32 : 48);

/** Whether the effective tier is a paid one (absent until a licence is applied: Free). */
export const isPaidTier = (tier: string | undefined) => tier === "basic" || tier === "premium";

/** Wraps one line at word boundaries; a word longer than `width` is cut at `width`. */
export function wrapLine(line: string, width: number): string[] {
  const out: string[] = [];
  let current = "";
  for (const word of line.split(/\s+/).filter(Boolean)) {
    let rest = word;
    while (rest.length > width) {
      if (current) {
        out.push(current);
        current = "";
      }
      out.push(rest.slice(0, width));
      rest = rest.slice(width);
    }
    if (!current) current = rest;
    else if (current.length + 1 + rest.length <= width) current += ` ${rest}`;
    else {
      out.push(current);
      current = rest;
    }
  }
  if (current) out.push(current);
  return out;
}

/** Wraps every line of `text` (lines are separated by a newline); blank lines are dropped. */
export function wrapText(text: string, width: number): string[] {
  return text.split("\n").flatMap((line) => wrapLine(line, width));
}

const centre = (line: string, width: number) =>
  " ".repeat(Math.max(0, Math.floor((width - line.length) / 2))) + line;

export type PreviewInput = {
  paperMm: string;
  name: string;
  address: string;
  phone: string;
  /** Printed under the phone for a GST branch with a GSTIN; empty otherwise. */
  gstin: string;
  footer: string;
  /** The text between header and footer, which the layout stories fill in. */
  body: string;
};

/** The receipt as plain text at the chosen paper width. */
export function previewReceipt(i: PreviewInput): string[] {
  const width = lineWidth(i.paperMm);
  const header = [
    ...wrapText(i.name, width),
    ...wrapText(i.address, width),
    ...wrapText(i.phone, width),
    // The statutory label printed on a GST receipt.
    ...(i.gstin ? wrapText(`GSTIN ${i.gstin}`, width) : []), // i18n-ignore
  ].map((l) => centre(l, width));
  const rule = "-".repeat(width);
  return [
    ...header,
    rule,
    ...wrapText(i.body, width),
    rule,
    ...wrapText(i.footer, width).map((l) => centre(l, width)),
  ];
}
