// GenTactix V2 frontend · 零 404 美术贴图安全加载器（Vite 零配置丢图 + manifest.json 运行时覆盖）
// 原则：只有当文件真实存在于 `src/assets/` 或 `public/assets/manifest.json` 显式声明时才发起图片加载，
//       未摆放的槽位绝不发 404 请求，零 console.error，直接安全回退程序化 Canvas 绘制。

import { ART_ASSET_MANIFEST, ART_SLOT_LIST, type ArtSlotKey, type ArtSlotSpec } from './assetManifest';

export type ArtLoadStatus = 'idle' | 'loading' | 'loaded' | 'error';

interface CacheEntry {
  status: ArtLoadStatus;
  img: HTMLImageElement | null;
  resolvedUrl?: string;
}

const bundledArtModules = import.meta.glob('../assets/**/*.{png,webp,svg}', {
  eager: true,
  query: '?url',
  import: 'default',
}) as Record<string, string>;

const cache = new Map<ArtSlotKey, CacheEntry>();
const listeners = new Set<() => void>();
let manifestOverrides: Partial<Record<ArtSlotKey, string>> = {};
let cacheBustSeq = 0;

function notifyListeners(): void {
  for (const fn of listeners) {
    try {
      fn();
    } catch {
      // ignore listener errors
    }
  }
}

export function subscribeArtChanges(fn: () => void): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

function findBundledUrl(spec: ArtSlotSpec): string | undefined {
  const relWithExt = spec.url.replace(/^\/assets\//, '');
  const relStem = relWithExt.replace(/\.[^.]+$/, '');
  for (const [modPath, assetUrl] of Object.entries(bundledArtModules)) {
    const norm = modPath.replace(/\\/g, '/');
    if (norm.endsWith(`/${relWithExt}`)) return assetUrl;
    for (const ext of ['webp', 'svg', 'png']) {
      if (norm.endsWith(`/${relStem}.${ext}`)) return assetUrl;
    }
  }
  return undefined;
}

function resolveSlotUrl(spec: ArtSlotSpec): string | undefined {
  const overrideUrl = manifestOverrides[spec.key];
  if (typeof overrideUrl === 'string' && overrideUrl.trim().length > 0) {
    return overrideUrl.trim();
  }
  return findBundledUrl(spec);
}

function loadSlotFromUrl(spec: ArtSlotSpec, url: string, forceBust: boolean): void {
  if (typeof Image === 'undefined') {
    cache.set(spec.key, { status: 'idle', img: null, resolvedUrl: url });
    return;
  }
  const existing = cache.get(spec.key);
  if (!forceBust && existing && existing.resolvedUrl === url && (existing.status === 'loading' || existing.status === 'loaded')) {
    return;
  }
  const img = new Image();
  cache.set(spec.key, { status: 'loading', img: null, resolvedUrl: url });
  img.onload = () => {
    if (img.naturalWidth > 0) {
      cache.set(spec.key, { status: 'loaded', img, resolvedUrl: url });
    } else {
      cache.set(spec.key, { status: 'error', img: null, resolvedUrl: url });
    }
    notifyListeners();
  };
  img.onerror = () => {
    cache.set(spec.key, { status: 'error', img: null, resolvedUrl: url });
    notifyListeners();
  };
  const sep = url.includes('?') ? '&' : '?';
  img.src = cacheBustSeq > 0 ? `${url}${sep}v=${cacheBustSeq}` : url;
}

function syncSlots(forceBust: boolean): void {
  for (const spec of ART_SLOT_LIST) {
    const url = resolveSlotUrl(spec);
    if (!url) {
      cache.set(spec.key, { status: 'idle', img: null });
      continue;
    }
    loadSlotFromUrl(spec, url, forceBust);
  }
}

async function fetchManifestOverrides(): Promise<void> {
  if (typeof window === 'undefined' || typeof fetch !== 'function') return;
  if (!window.location?.protocol?.startsWith('http')) return;
  const isVitest = typeof import.meta !== 'undefined' && (import.meta as { env?: Record<string, unknown> }).env?.MODE === 'test';
  if (isVitest) return;
  try {
    const url = cacheBustSeq > 0 ? `/assets/manifest.json?v=${cacheBustSeq}` : '/assets/manifest.json';
    const res = await fetch(url);
    if (!res.ok) return;
    const json = (await res.json()) as { overrides?: Partial<Record<ArtSlotKey, string>> };
    if (json?.overrides && typeof json.overrides === 'object') {
      manifestOverrides = json.overrides;
    }
  } catch {
    // 静态 manifest 不存在或网络不可达时静默回退程序化矢量，绝不报 404 console.error
  }
}

/** 同步读取已就绪的槽位图片；未放置文件、加载中或失败时统一返回 null（安全回退程序化绘制）。 */
export function getArtImage(key: ArtSlotKey): HTMLImageElement | null {
  if (typeof Image === 'undefined') return null;
  const entry = cache.get(key);
  if (!entry || entry.status !== 'loaded' || !entry.img) return null;
  if (!entry.img.complete || entry.img.naturalWidth <= 0) return null;
  return entry.img;
}

/** 预加载所有已实际放入 `src/assets/` 或在 `public/assets/manifest.json` 声明覆盖的槽位；其余槽位静默零请求兜底。 */
export function preloadArtAssets(): void {
  syncSlots(false);
  void fetchManifestOverrides().then(() => {
    syncSlots(false);
    notifyListeners();
  });
}

/** 美术热重载按钮专用：刷新 manifest.json 并重载全部已命中贴图槽位。 */
export function reloadArtAssets(): void {
  cacheBustSeq += 1;
  cache.clear();
  syncSlots(true);
  void fetchManifestOverrides().then(() => {
    syncSlots(true);
    notifyListeners();
  });
  notifyListeners();
}

export function getLoadedArtCount(): number {
  let count = 0;
  for (const spec of ART_SLOT_LIST) {
    if (cache.get(spec.key)?.status === 'loaded') count += 1;
  }
  return count;
}

export function getArtSlotStatuses(): Array<{ spec: ArtSlotSpec; status: ArtLoadStatus }> {
  return ART_SLOT_LIST.map((spec) => ({
    spec,
    status: cache.get(spec.key)?.status ?? 'idle',
  }));
}

export { ART_ASSET_MANIFEST };
