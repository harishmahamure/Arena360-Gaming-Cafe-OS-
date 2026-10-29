// The advances this display could not send (Story 7.10): kept in IndexedDB so a reload during LAN
// loss keeps them, and replayed in creation order when the Hub is back. Each carries the ULID
// `idempotency_key` minted at the tap, so the Hub applies it once however often it is sent.

/** How long an unsent advance is worth sending: 24 hours. */
export const QUEUE_TTL_MS = 24 * 60 * 60_000;

/** One advance waiting for the Hub. */
export type Queued = {
  /** The idempotency key, minted at the tap. */
  key: string;
  ticketId: string;
  /** The stage the tap moved the ticket to. */
  to: string;
  /** The rev the ticket is predicted to have when the Hub reaches this move. */
  baseRev: number;
  /** When it was tapped (tablet clock, milliseconds): the intent's `issued_at`. */
  issuedAt: number;
  /** When the display gives up on it. */
  expiresAt: number;
};

const DB_NAME = "arena.kitchen"; // i18n-ignore (a store name)
const STORE = "queue"; // i18n-ignore (a store name)

/** The queue's storage: a few async calls, so the page can run on a memory store when blocked. */
export type QueueStore = {
  load: () => Promise<Queued[]>;
  put: (entry: Queued) => Promise<void>;
  remove: (key: string) => Promise<void>;
};

/** The entries still worth sending at `now`, in creation order. */
export function liveEntries(entries: readonly Queued[], now: number): Queued[] {
  return entries.filter((e) => e.expiresAt > now).sort((a, b) => a.issuedAt - b.issuedAt);
}

/** The rev a new advance of `ticketId` is based on: the Hub's rev plus the moves already queued. */
export function predictedRev(queue: readonly Queued[], ticketId: string, hubRev: number): number {
  return hubRev + queue.filter((e) => e.ticketId === ticketId).length;
}

/** The queue without the entry `key` (an Undo of a move that was never sent, or one that was settled). */
export function without(queue: readonly Queued[], key: string): Queued[] {
  return queue.filter((e) => e.key !== key);
}

/** A store that lives only as long as the page (storage blocked, or no IndexedDB). */
export function memoryStore(): QueueStore {
  const held = new Map<string, Queued>();
  return {
    load: async () => [...held.values()],
    put: async (entry) => void held.set(entry.key, entry),
    remove: async (key) => void held.delete(key),
  };
}

/** The IndexedDB store, or the memory store when the browser will not open one. */
export function openQueueStore(): QueueStore {
  let opened: Promise<IDBDatabase> | null = null;
  const db = () => {
    opened ??= new Promise<IDBDatabase>((resolve, reject) => {
      const open = globalThis.indexedDB.open(DB_NAME, 1);
      open.onupgradeneeded = () => {
        open.result.createObjectStore(STORE, { keyPath: "key" });
      };
      open.onsuccess = () => resolve(open.result);
      open.onerror = () => reject(open.error);
      open.onblocked = () => reject(new Error("blocked"));
    });
    return opened;
  };
  const fallback = memoryStore();
  let broken = typeof globalThis.indexedDB === "undefined";
  // Any storage failure moves this page to the memory store for good: the board keeps working.
  const guard = async <T>(
    use: (store: IDBObjectStore) => IDBRequest<T>,
    mode: IDBTransactionMode,
  ): Promise<T> => {
    const handle = await db();
    return new Promise<T>((resolve, reject) => {
      const tx = handle.transaction(STORE, mode);
      const req = use(tx.objectStore(STORE));
      // Resolve when the transaction is committed, so a reload right after a tap keeps the move.
      tx.oncomplete = () => resolve(req.result);
      tx.onerror = tx.onabort = () => reject(tx.error);
    });
  };
  return {
    load: async () => {
      if (!broken) {
        try {
          return await guard((s) => s.getAll() as IDBRequest<Queued[]>, "readonly");
        } catch {
          broken = true;
        }
      }
      return fallback.load();
    },
    put: async (entry) => {
      if (!broken) {
        try {
          await guard((s) => s.put(entry), "readwrite");
          return;
        } catch {
          broken = true;
        }
      }
      await fallback.put(entry);
    },
    remove: async (key) => {
      if (!broken) {
        try {
          await guard((s) => s.delete(key), "readwrite");
          return;
        } catch {
          broken = true;
        }
      }
      await fallback.remove(key);
    },
  };
}
