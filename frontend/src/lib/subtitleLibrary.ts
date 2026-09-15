export interface SavedSubtitle {
  id: string;
  itemId: string;
  name: string;
  language: string;
  vtt: string;
}

function openDatabase(name: string): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(name, 1);
    request.onupgradeneeded = () => {
      if (request.result.objectStoreNames.contains('subtitles')) return;
      const store = request.result.createObjectStore('subtitles', { keyPath: 'id' });
      store.createIndex('itemId', 'itemId');
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

let migration: Promise<void> | undefined;

function migrateSavedSubtitles(): Promise<void> {
  if (localStorage.getItem('jellymax-subtitles-migrated')) return Promise.resolve();
  migration ??= (async () => {
    // Keep subtitles imported before the application was renamed.
    const oldDb = await openDatabase('jellyfin-rust-subtitles');
    let saved: SavedSubtitle[];
    try {
      saved = await new Promise((resolve, reject) => {
        const request = oldDb.transaction('subtitles', 'readonly').objectStore('subtitles').getAll();
        request.onsuccess = () => resolve(request.result as SavedSubtitle[]);
        request.onerror = () => reject(request.error);
      });
    } finally { oldDb.close(); }
    if (saved.length) {
      const db = await openDatabase('jellymax-subtitles');
      try {
        await new Promise<void>((resolve, reject) => {
          const transaction = db.transaction('subtitles', 'readwrite');
          const store = transaction.objectStore('subtitles');
          for (const subtitle of saved) store.put(subtitle);
          transaction.oncomplete = () => resolve();
          transaction.onerror = () => reject(transaction.error);
          transaction.onabort = () => reject(transaction.error);
        });
      } finally { db.close(); }
    }
    localStorage.setItem('jellymax-subtitles-migrated', '1');
    indexedDB.deleteDatabase('jellyfin-rust-subtitles');
  })().catch(error => { migration = undefined; throw error; });
  return migration;
}

async function openLibrary(): Promise<IDBDatabase> {
  await migrateSavedSubtitles();
  return openDatabase('jellymax-subtitles');
}

export async function listSavedSubtitles(itemId: string): Promise<SavedSubtitle[]> {
  const db = await openLibrary();
  try {
    return await new Promise((resolve, reject) => {
      const request = db.transaction('subtitles', 'readonly').objectStore('subtitles').index('itemId').getAll(itemId);
      request.onsuccess = () => resolve(request.result as SavedSubtitle[]);
      request.onerror = () => reject(request.error);
    });
  } finally { db.close(); }
}

export async function saveSubtitle(subtitle: SavedSubtitle): Promise<void> {
  const db = await openLibrary();
  try {
    await new Promise<void>((resolve, reject) => {
      const request = db.transaction('subtitles', 'readwrite').objectStore('subtitles').put(subtitle);
      request.onsuccess = () => resolve();
      request.onerror = () => reject(request.error);
    });
  } finally { db.close(); }
}

export async function deleteSubtitle(id: string): Promise<void> {
  const db = await openLibrary();
  try {
    await new Promise<void>((resolve, reject) => {
      const request = db.transaction('subtitles', 'readwrite').objectStore('subtitles').delete(id);
      request.onsuccess = () => resolve();
      request.onerror = () => reject(request.error);
    });
  } finally { db.close(); }
}
