const DB = "arena-owner";
const STORE = "payloads";

export type Cached<T> = { payload: T; as_of: string };

function open(): Promise<IDBDatabase> {
  return new Promise((ok, fail) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => ok(req.result);
    req.onerror = () => fail(req.error);
  });
}

async function run<T>(mode: IDBTransactionMode, f: (s: IDBObjectStore) => IDBRequest<T>) {
  try {
    const db = await open();
    return await new Promise<T | undefined>((ok) => {
      const req = f(db.transaction(STORE, mode).objectStore(STORE));
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
export const clearCache = () => run("readwrite", (s) => s.clear());
