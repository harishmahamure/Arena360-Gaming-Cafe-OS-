const DB = "arena-owner";
const STORE = "payloads";
/** Commands made with no signal, kept by idempotency key (Story 8.22). */
const QUEUE = "queue";

export type Cached<T> = { payload: T; as_of: string };

function open(): Promise<IDBDatabase> {
  return new Promise((ok, fail) => {
    const req = indexedDB.open(DB, 2);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE);
      if (!db.objectStoreNames.contains(QUEUE)) db.createObjectStore(QUEUE, { keyPath: "key" });
    };
    req.onsuccess = () => ok(req.result);
    req.onerror = () => fail(req.error);
  });
}

async function run<T>(
  mode: IDBTransactionMode,
  f: (s: IDBObjectStore) => IDBRequest<T>,
  store = STORE,
) {
  try {
    const db = await open();
    return await new Promise<T | undefined>((ok) => {
      const req = f(db.transaction(store, mode).objectStore(store));
      req.onsuccess = () => ok(req.result);
      req.onerror = () => ok(undefined);
    });
  } catch {
    return undefined;
  }
}

export const getCached = <T>(key: string) =>
  run("readonly", (s) => s.get(key)) as Promise<Cached<T> | undefined>;
export const putCached = <T>(key: string, payload: T) =>
  run("readwrite", (s) => s.put({ payload, as_of: new Date().toISOString() }, key));
/** Clears the cached screens; the queue of unsent commands is a separate store and stays. */
export const clearCache = () => run("readwrite", (s) => s.clear());

export const queueAll = <T>() =>
  run("readonly", (s) => s.getAll(), QUEUE) as Promise<T[] | undefined>;
export const queuePut = <T extends { key: string }>(item: T) =>
  run("readwrite", (s) => s.put(item), QUEUE);
export const queueDelete = (key: string) => run("readwrite", (s) => s.delete(key), QUEUE);
