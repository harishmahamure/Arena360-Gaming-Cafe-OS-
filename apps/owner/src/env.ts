import { defineEnvVars } from "@sveltejs/kit/env";

export const variables = defineEnvVars({
  PUBLIC_API_BASE: { public: true, schema: (value) => value ?? "" },
  PUBLIC_ONBOARDING_URL: {
    public: true,
    schema: (value) => value || "https://start.arena360.cloud",
  },
  PUBLIC_SUPPORT_EMAIL: { public: true, schema: (value) => value || "support@arena360.cloud" },
});
