import { defineEnvVars } from "@sveltejs/kit/env";

export const variables = defineEnvVars({
  PUBLIC_ANDROID_HUB_URL: {
    public: true,
    schema: (value) => value || "https://arena360.cloud/download/android",
  },
});
