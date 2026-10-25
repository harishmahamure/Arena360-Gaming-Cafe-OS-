import { defineEnvVars } from "@sveltejs/kit/env";

// `PUBLIC_API_BASE`: the API origin (`https://api.arena360.cloud`), read when the app starts.
// Empty or unset means the same origin, which the dev and preview proxy sends to the local
// `arena-cloud --role all --dev`.
export const variables = defineEnvVars({
  PUBLIC_API_BASE: {
    public: true,
    schema: (value) => value ?? "",
  },
  PUBLIC_HUB_DOWNLOAD_URL: {
    public: true,
    schema: (value) => value || "/download/hub",
  },
});
