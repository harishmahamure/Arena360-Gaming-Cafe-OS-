// Razorpay Checkout for a plan change that needs a new mandate (Story 10.9): loads checkout.js on
// demand and builds the options from the parameters the cloud answered. Only the methods the cloud
// allows are offered; the default blocks (wallet, netbanking, pay later, EMI) are switched off.

import { t } from "@arena/ui";
import type { SubscriptionCheckout } from "@arena/ui/generated";

export const CHECKOUT_SRC = "https://checkout.razorpay.com/v1/checkout.js";

export type RazorpayOptions = {
  key: string;
  subscription_id: string;
  name: string;
  description: string;
  handler: (response: unknown) => void;
  modal: { ondismiss: () => void; escape: boolean; confirm_close: boolean };
  config: {
    display: {
      blocks: Record<string, { name: string; instruments: { method: string }[] }>;
      sequence: string[];
      preferences: { show_default_blocks: boolean };
    };
  };
};

export interface RazorpayInstance {
  open(): void;
  close(): void;
  on(event: "payment.failed", handler: (response: unknown) => void): void;
}

export type RazorpayConstructor = new (options: RazorpayOptions) => RazorpayInstance;

declare global {
  interface Window {
    Razorpay?: RazorpayConstructor;
  }
}

let loading: Promise<RazorpayConstructor> | null = null;

/** `window.Razorpay`, injecting checkout.js the first time it is needed. */
export function loadRazorpay(): Promise<RazorpayConstructor> {
  if (window.Razorpay) return Promise.resolve(window.Razorpay);
  loading ??= new Promise<RazorpayConstructor>((resolve, reject) => {
    const script = document.createElement("script");
    script.src = CHECKOUT_SRC;
    script.async = true;
    script.onload = () => {
      if (window.Razorpay) resolve(window.Razorpay);
      else reject(new Error("no-razorpay"));
    };
    script.onerror = () => {
      script.remove();
      reject(new Error("script-failed"));
    };
    document.head.append(script);
  }).catch((e: unknown) => {
    loading = null;
    throw e;
  });
  return loading;
}

/** The Checkout options for the new mandate: its key, its Razorpay id and the allowed methods. */
export function checkoutOptions(
  checkout: SubscriptionCheckout,
  plan: string,
  handlers: { paid: () => void; dismissed: () => void },
): RazorpayOptions {
  return {
    key: checkout.key_id,
    subscription_id: checkout.razorpay_subscription_id,
    name: t("app.name"),
    description: t("owner.subscription.change.pay_description", { plan }),
    handler: () => handlers.paid(),
    modal: { ondismiss: () => handlers.dismissed(), escape: true, confirm_close: false },
    config: {
      display: {
        blocks: {
          allowed: {
            name: t("owner.subscription.change.pay_methods"),
            instruments: checkout.methods.map((method) => ({ method })),
          },
        },
        sequence: ["block.allowed"],
        preferences: { show_default_blocks: false },
      },
    },
  };
}
