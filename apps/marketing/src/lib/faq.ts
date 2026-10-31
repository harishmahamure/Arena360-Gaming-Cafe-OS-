// The FAQ's content map and its `FAQPage` structured data. Pure, with no imports, so the script
// tests can load it too. The words are catalogue keys: the offline, cancellation and refund answers
// are the Pricing FAQ's own keys, so the two pages can never say different things.

export interface FaqItem {
  /** Catalogue keys of the question and its answer. */
  q: string;
  a: string;
}

export interface FaqGroup {
  id: string;
  /** Catalogue key of the group heading. */
  title: string;
  items: FaqItem[];
}

const item = (prefix: string): FaqItem => ({ q: `${prefix}.q`, a: `${prefix}.a` }); // i18n-ignore

export const FAQ_GROUPS: FaqGroup[] = [
  {
    id: "billing",
    title: "site.faq.group.billing",
    items: [item("site.faq.billing.charge"), item("site.pricing.faq.cancel")],
  },
  {
    id: "data",
    title: "site.faq.group.data",
    items: [item("site.faq.data.export"), item("site.faq.data.where"), item("site.faq.data.cloud")],
  },
  { id: "offline", title: "site.faq.group.offline", items: [item("site.pricing.faq.offline")] },
  {
    id: "ads",
    title: "site.faq.group.ads",
    items: [item("site.faq.ads.where"), item("site.faq.ads.remove")],
  },
  { id: "refunds", title: "site.faq.group.refunds", items: [item("site.pricing.faq.refund")] },
];

/** The `FAQPage` JSON-LD for every question on the page, in page order. */
export function faqJsonLd(groups: FaqGroup[], say: (key: string) => string) {
  return {
    "@context": "https://schema.org",
    "@type": "FAQPage",
    mainEntity: groups.flatMap((group) =>
      group.items.map((entry) => ({
        "@type": "Question",
        name: say(entry.q),
        acceptedAnswer: { "@type": "Answer", text: say(entry.a) },
      })),
    ),
  };
}
