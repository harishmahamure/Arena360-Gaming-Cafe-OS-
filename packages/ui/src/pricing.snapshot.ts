// Generated from the seed price book by `UPDATE_PRICING_SNAPSHOT=1 cargo test -p arena-cloud
// --test pricing_snapshot` (Story 10.1). Do not edit: CI fails when it differs from the book.
export const PRICING_SNAPSHOT = {
  "bands": {
    "IN": {
      "currency": "INR",
      "enabled": true,
      "lines": [
        {
          "amount_minor": 5000,
          "cycle": "monthly",
          "gst_applicable": true,
          "item": "addon_devices_20"
        },
        {
          "amount_minor": 50000,
          "cycle": "yearly",
          "gst_applicable": true,
          "item": "addon_devices_20"
        },
        {
          "amount_minor": 5000,
          "cycle": "monthly",
          "gst_applicable": true,
          "item": "addon_owner_login"
        },
        {
          "amount_minor": 50000,
          "cycle": "yearly",
          "gst_applicable": true,
          "item": "addon_owner_login"
        },
        {
          "amount_minor": 20000,
          "cycle": "monthly",
          "gst_applicable": true,
          "item": "addon_tunnel"
        },
        {
          "amount_minor": 200000,
          "cycle": "yearly",
          "gst_applicable": true,
          "item": "addon_tunnel"
        },
        {
          "amount_minor": 20000,
          "cycle": "monthly",
          "gst_applicable": true,
          "item": "basic"
        },
        {
          "amount_minor": 200000,
          "cycle": "yearly",
          "gst_applicable": true,
          "item": "basic"
        },
        {
          "amount_minor": 199900,
          "cycle": "monthly",
          "gst_applicable": true,
          "item": "premium"
        },
        {
          "amount_minor": 1999000,
          "cycle": "yearly",
          "gst_applicable": true,
          "item": "premium"
        }
      ]
    },
    "MY": {
      "currency": "MYR",
      "enabled": false,
      "lines": [
        {
          "amount_minor": 600,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "addon_devices_20"
        },
        {
          "amount_minor": 6000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "addon_devices_20"
        },
        {
          "amount_minor": 600,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "addon_owner_login"
        },
        {
          "amount_minor": 6000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "addon_owner_login"
        },
        {
          "amount_minor": 2300,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "addon_tunnel"
        },
        {
          "amount_minor": 23000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "addon_tunnel"
        },
        {
          "amount_minor": 2300,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "basic"
        },
        {
          "amount_minor": 23000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "basic"
        },
        {
          "amount_minor": 22900,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "premium"
        },
        {
          "amount_minor": 229000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "premium"
        }
      ]
    },
    "SG": {
      "currency": "SGD",
      "enabled": false,
      "lines": [
        {
          "amount_minor": 200,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "addon_devices_20"
        },
        {
          "amount_minor": 2000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "addon_devices_20"
        },
        {
          "amount_minor": 200,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "addon_owner_login"
        },
        {
          "amount_minor": 2000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "addon_owner_login"
        },
        {
          "amount_minor": 800,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "addon_tunnel"
        },
        {
          "amount_minor": 8000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "addon_tunnel"
        },
        {
          "amount_minor": 800,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "basic"
        },
        {
          "amount_minor": 8000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "basic"
        },
        {
          "amount_minor": 7900,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "premium"
        },
        {
          "amount_minor": 79000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "premium"
        }
      ]
    },
    "US": {
      "currency": "USD",
      "enabled": false,
      "lines": [
        {
          "amount_minor": 150,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "addon_devices_20"
        },
        {
          "amount_minor": 1500,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "addon_devices_20"
        },
        {
          "amount_minor": 150,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "addon_owner_login"
        },
        {
          "amount_minor": 1500,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "addon_owner_login"
        },
        {
          "amount_minor": 600,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "addon_tunnel"
        },
        {
          "amount_minor": 6000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "addon_tunnel"
        },
        {
          "amount_minor": 600,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "basic"
        },
        {
          "amount_minor": 6000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "basic"
        },
        {
          "amount_minor": 5900,
          "cycle": "monthly",
          "gst_applicable": false,
          "item": "premium"
        },
        {
          "amount_minor": 59000,
          "cycle": "yearly",
          "gst_applicable": false,
          "item": "premium"
        }
      ]
    }
  },
  "version": 1
} as const;
