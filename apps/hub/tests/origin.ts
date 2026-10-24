const env = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env;

export const ORIGIN = `http://127.0.0.1:${env?.ARENA_PW_PORT ?? 5173}`;
