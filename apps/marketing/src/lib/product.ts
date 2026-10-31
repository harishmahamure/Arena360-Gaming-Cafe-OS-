import type { MessageKey } from "@arena/ui/messages";
import { mediaUrl } from "./site";

// The Product page's content: four parts, each with an illustrated walkthrough and one demo video.
// Words live in the message catalogue (`site.product.*`); this file is the structure and the media.

export const PRODUCT_IDS = ["hub", "kiosk", "console", "owner"] as const;
export type ProductId = (typeof PRODUCT_IDS)[number];

/** The small illustration drawn above a step (`StepArt.astro`). */
export type Art =
  | "install"
  | "pair"
  | "floor"
  | "bill"
  | "offline"
  | "screen"
  | "login"
  | "plan"
  | "hud"
  | "console"
  | "timer"
  | "phone"
  | "chart"
  | "sync";

export interface Step {
  art: Art;
  title: MessageKey;
  body: MessageKey;
}

export interface Product {
  id: ProductId;
  title: MessageKey;
  lede: MessageKey;
  jump: MessageKey;
  steps: Step[];
  /** A plain note under the steps (Console says what is and is not there yet). */
  note?: MessageKey;
}

export const PRODUCTS: Product[] = [
  {
    id: "hub",
    title: "site.product.hub.title",
    lede: "site.product.hub.lede",
    jump: "site.product.jump.hub",
    steps: [
      { art: "install", title: "site.product.hub.s1.title", body: "site.product.hub.s1.body" },
      { art: "pair", title: "site.product.hub.s2.title", body: "site.product.hub.s2.body" },
      { art: "floor", title: "site.product.hub.s3.title", body: "site.product.hub.s3.body" },
      { art: "bill", title: "site.product.hub.s4.title", body: "site.product.hub.s4.body" },
      { art: "offline", title: "site.product.hub.s5.title", body: "site.product.hub.s5.body" },
    ],
  },
  {
    id: "kiosk",
    title: "site.product.kiosk.title",
    lede: "site.product.kiosk.lede",
    jump: "site.product.jump.kiosk",
    steps: [
      { art: "screen", title: "site.product.kiosk.s1.title", body: "site.product.kiosk.s1.body" },
      { art: "login", title: "site.product.kiosk.s2.title", body: "site.product.kiosk.s2.body" },
      { art: "plan", title: "site.product.kiosk.s3.title", body: "site.product.kiosk.s3.body" },
      { art: "hud", title: "site.product.kiosk.s4.title", body: "site.product.kiosk.s4.body" },
    ],
  },
  {
    id: "console",
    title: "site.product.console.title",
    lede: "site.product.console.lede",
    jump: "site.product.jump.console",
    steps: [
      {
        art: "console",
        title: "site.product.console.s1.title",
        body: "site.product.console.s1.body",
      },
      {
        art: "floor",
        title: "site.product.console.s2.title",
        body: "site.product.console.s2.body",
      },
      {
        art: "timer",
        title: "site.product.console.s3.title",
        body: "site.product.console.s3.body",
      },
    ],
    note: "site.product.console.note",
  },
  {
    id: "owner",
    title: "site.product.owner.title",
    lede: "site.product.owner.lede",
    jump: "site.product.jump.owner",
    steps: [
      { art: "phone", title: "site.product.owner.s1.title", body: "site.product.owner.s1.body" },
      { art: "floor", title: "site.product.owner.s2.title", body: "site.product.owner.s2.body" },
      { art: "chart", title: "site.product.owner.s3.title", body: "site.product.owner.s3.body" },
      { art: "sync", title: "site.product.owner.s4.title", body: "site.product.owner.s4.body" },
    ],
  },
];

export interface DemoMedia {
  /** Still shown until the viewer presses play. */
  poster: string;
  /** Tried in order; the first the browser can play wins. */
  sources: { src: string; type: string }[];
  /** WebVTT English captions, on by default. */
  captions: string;
  /** The text transcript. */
  transcript: string;
}

/** A product's demo files under `marketing/product/<id>/` in the shared R2 prefix. */
export function demoMedia(id: ProductId): DemoMedia {
  const file = (name: string) => mediaUrl(`product/${id}/${name}`); // i18n-ignore
  return {
    poster: file("poster.jpg"),
    sources: [
      { src: file("demo.webm"), type: "video/webm" },
      { src: file("demo.mp4"), type: "video/mp4" },
    ],
    captions: file("captions.en.vtt"),
    transcript: file("transcript.txt"),
  };
}
