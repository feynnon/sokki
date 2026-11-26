/** Recordings kept on the phone until the Mac has them, plus their list. */

export interface Recording {
  localId: string;
  startedAt: string; // ISO
  durationS: number;
  remoteId: string | null; // the Mac's meeting id once uploaded
  state: "waiting" | "uploading" | "processing" | "done" | "failed";
  stage: string | null;
  error: string | null;
  title: string | null;
  summary: string | null;
  transcript: string | null;
}

const LIST_KEY = "verbatim.recordings";

export function loadList(): Recording[] {
  try {
    return JSON.parse(localStorage.getItem(LIST_KEY) ?? "[]");
  } catch {
    return [];
  }
}

export function saveList(list: Recording[]) {
  try {
    localStorage.setItem(LIST_KEY, JSON.stringify(list));
  } catch {
    /* storage full or blocked: the list lives on in memory */
  }
}

// Audio blobs in IndexedDB, so a recording the Mac didn't get survives
// closing the app until the user taps Check again.
const DB = "verbatim";
const STORE = "audio";

function db(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

export async function putAudio(id: string, blob: Blob) {
  const d = await db();
  await new Promise<void>((resolve, reject) => {
    const tx = d.transaction(STORE, "readwrite");
    tx.objectStore(STORE).put(blob, id);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

export async function getAudio(id: string): Promise<Blob | null> {
  const d = await db();
  return new Promise((resolve, reject) => {
    const req = d.transaction(STORE).objectStore(STORE).get(id);
    req.onsuccess = () => resolve((req.result as Blob) ?? null);
    req.onerror = () => reject(req.error);
  });
}

export async function deleteAudio(id: string) {
  const d = await db();
  await new Promise<void>((resolve) => {
    const tx = d.transaction(STORE, "readwrite");
    tx.objectStore(STORE).delete(id);
    tx.oncomplete = () => resolve();
    tx.onerror = () => resolve();
  });
}
