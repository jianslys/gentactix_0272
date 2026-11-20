// GenTactix V2 frontend · Layer 6 本地持久化（IndexedDB + localStorage 双写兜底）与 L7/L9 独立开关态
// 严禁经 BackendAdapter 发送 Layer 6 手绘数据：100% 仅保存在浏览器本地。

import type { Vec2 } from '../engine/types';

export type FreehandTool = 'none' | 'pen' | 'eraser';
export type WeatherMode = 'rain' | 'fog' | 'snow';

export interface FreehandStroke {
  id: string;
  color: string;
  width: number;
  points: Vec2[];
}

const DB_NAME = 'gentactix-v2-local';
const STORE_NAME = 'layer6';
const DB_KEY = 'strokes';
const LS_KEY = 'gentactix:v2:layer6:strokes';

function openDb(): Promise<IDBDatabase | null> {
  if (typeof indexedDB === 'undefined') return Promise.resolve(null);
  return new Promise((resolve) => {
    try {
      const req = indexedDB.open(DB_NAME, 1);
      req.onupgradeneeded = () => {
        const db = req.result;
        if (!db.objectStoreNames.contains(STORE_NAME)) {
          db.createObjectStore(STORE_NAME);
        }
      };
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
}

function writeLocalStorage(strokes: FreehandStroke[]): void {
  try {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(LS_KEY, JSON.stringify(strokes));
    }
  } catch {
    // 忽略配额异常
  }
}

function readLocalStorage(): FreehandStroke[] | null {
  try {
    if (typeof localStorage === 'undefined') return null;
    const raw = localStorage.getItem(LS_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as FreehandStroke[]) : null;
  } catch {
    return null;
  }
}

export async function saveLayer6Strokes(strokes: FreehandStroke[]): Promise<'indexeddb' | 'localstorage'> {
  writeLocalStorage(strokes);
  const db = await openDb();
  if (!db) return 'localstorage';
  return new Promise((resolve) => {
    try {
      const tx = db.transaction(STORE_NAME, 'readwrite');
      const store = tx.objectStore(STORE_NAME);
      store.put(strokes, DB_KEY);
      tx.oncomplete = () => {
        db.close();
        resolve('indexeddb');
      };
      tx.onerror = () => {
        db.close();
        resolve('localstorage');
      };
    } catch {
      db.close();
      resolve('localstorage');
    }
  });
}

export async function loadLayer6Strokes(): Promise<FreehandStroke[]> {
  const db = await openDb();
  if (db) {
    const idbData = await new Promise<FreehandStroke[] | null>((resolve) => {
      try {
        const tx = db.transaction(STORE_NAME, 'readonly');
        const store = tx.objectStore(STORE_NAME);
        const req = store.get(DB_KEY);
        req.onsuccess = () => {
          db.close();
          resolve(Array.isArray(req.result) ? (req.result as FreehandStroke[]) : null);
        };
        req.onerror = () => {
          db.close();
          resolve(null);
        };
      } catch {
        db.close();
        resolve(null);
      }
    });
    if (idbData && idbData.length > 0) return idbData;
    const lsFallback = readLocalStorage();
    if (lsFallback && lsFallback.length > 0) return lsFallback;
    if (idbData) return idbData;
  }
  return readLocalStorage() ?? [];
}

export async function clearLayer6Strokes(): Promise<void> {
  await saveLayer6Strokes([]);
}
