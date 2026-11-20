// GenTactix V2 frontend · 浮动窗口状态树（Docked-by-Default -> Detach-on-Drag / localStorage 持久化）

export type ManagedWindowId = 'W2' | 'W3' | 'W4' | 'W5' | 'W7' | 'W8';

export const MANAGED_WINDOW_IDS: ManagedWindowId[] = ['W2', 'W3', 'W4', 'W5', 'W7', 'W8'];

export interface WindowState {
  id: ManagedWindowId;
  title: string;
  shortLabel: string;
  sourceFile: string;
  floating: boolean;
  x: number;
  y: number;
  width: number;
  anchorRight?: boolean;
  open: boolean;
  minimized: boolean;
  zIndex: number;
}

export type WindowLayoutMap = Record<ManagedWindowId, WindowState>;

const STORAGE_KEY = 'gentactix_window_layout_v1';

export function createDefaultWindowLayout(): WindowLayoutMap {
  return {
    W2: {
      id: 'W2',
      title: 'UI-02 · L6 战术画笔（本地）',
      shortLabel: '画笔',
      sourceFile: 'src/ui/FreehandToolbar.tsx',
      floating: false,
      x: 8,
      y: 54,
      width: 310,
      open: true,
      minimized: false,
      zIndex: 21,
    },
    W3: {
      id: 'W3',
      title: 'UI-03 · 单位态势检视',
      shortLabel: '部队',
      sourceFile: 'src/ui/UnitInspector.tsx',
      floating: false,
      x: 8,
      y: 260,
      width: 310,
      open: true,
      minimized: false,
      zIndex: 22,
    },
    W4: {
      id: 'W4',
      title: 'UI-04 · 16s 战况日志',
      shortLabel: '日志',
      sourceFile: 'src/ui/UnitInspector.tsx',
      floating: false,
      x: 8,
      y: 520,
      width: 310,
      open: true,
      minimized: false,
      zIndex: 23,
    },
    W5: {
      id: 'W5',
      title: '战术手写板',
      shortLabel: '手写板',
      sourceFile: 'src/ui/UioClipboardPanel.tsx',
      floating: false,
      x: 880,
      y: 14,
      width: 420,
      anchorRight: true,
      open: true,
      minimized: false,
      zIndex: 24,
    },
    W7: {
      id: 'W7',
      title: '保姆美术与真后端面板',
      shortLabel: '开发',
      sourceFile: 'src/ui/DevArtPanel.tsx',
      floating: true,
      x: 274,
      y: 64,
      width: 430,
      open: false,
      minimized: false,
      zIndex: 30,
    },
    W8: {
      id: 'W8',
      title: '示例扩展窗口（复制即用）',
      shortLabel: '示例',
      sourceFile: 'src/ui/ExampleCustomWindow.tsx',
      floating: true,
      x: 310,
      y: 120,
      width: 340,
      open: false,
      minimized: false,
      zIndex: 31,
    },
  };
}

export function loadWindowLayout(): WindowLayoutMap {
  const defaults = createDefaultWindowLayout();
  if (typeof window === 'undefined' || typeof window.localStorage === 'undefined') {
    return defaults;
  }
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return defaults;
    const parsed = JSON.parse(raw) as Partial<Record<ManagedWindowId, Partial<WindowState>>>;
    for (const id of MANAGED_WINDOW_IDS) {
      const saved = parsed[id];
      if (!saved || typeof saved !== 'object') continue;
      defaults[id] = {
        ...defaults[id],
        floating: typeof saved.floating === 'boolean' ? saved.floating : defaults[id].floating,
        x: typeof saved.x === 'number' && Number.isFinite(saved.x) ? saved.x : defaults[id].x,
        y: typeof saved.y === 'number' && Number.isFinite(saved.y) ? saved.y : defaults[id].y,
        width:
          typeof saved.width === 'number' && Number.isFinite(saved.width) && saved.width >= 180
            ? saved.width
            : defaults[id].width,
        anchorRight: typeof saved.anchorRight === 'boolean' ? saved.anchorRight : defaults[id].anchorRight,
        open: typeof saved.open === 'boolean' ? saved.open : defaults[id].open,
        minimized: typeof saved.minimized === 'boolean' ? saved.minimized : defaults[id].minimized,
        zIndex: typeof saved.zIndex === 'number' && Number.isFinite(saved.zIndex) ? saved.zIndex : defaults[id].zIndex,
      };
    }
    return defaults;
  } catch {
    return createDefaultWindowLayout();
  }
}

export function saveWindowLayout(layout: WindowLayoutMap): void {
  if (typeof window === 'undefined' || typeof window.localStorage === 'undefined') return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(layout));
  } catch {
    // ignore quota errors
  }
}

export function clearWindowLayout(): void {
  if (typeof window === 'undefined' || typeof window.localStorage === 'undefined') return;
  try {
    window.localStorage.removeItem(STORAGE_KEY);
  } catch {
    // ignore storage errors
  }
}

export type WindowAction =
  | { type: 'FOCUS'; id: ManagedWindowId }
  | { type: 'MOVE'; id: ManagedWindowId; x: number; y: number }
  | { type: 'DOCK'; id: ManagedWindowId }
  | { type: 'CLOSE'; id: ManagedWindowId }
  | { type: 'OPEN'; id: ManagedWindowId }
  | { type: 'TOGGLE_OPEN'; id: ManagedWindowId }
  | { type: 'TOGGLE_MINIMIZE'; id: ManagedWindowId }
  | { type: 'RESET' };

function maxZ(layout: WindowLayoutMap): number {
  return MANAGED_WINDOW_IDS.reduce((acc, id) => Math.max(acc, layout[id].zIndex), 20);
}

export function windowReducer(state: WindowLayoutMap, action: WindowAction): WindowLayoutMap {
  switch (action.type) {
    case 'RESET':
      return createDefaultWindowLayout();
    case 'FOCUS': {
      const top = maxZ(state);
      const cur = state[action.id];
      if (cur.zIndex >= top) return state;
      return {
        ...state,
        [action.id]: { ...cur, zIndex: top + 1 },
      };
    }
    case 'MOVE': {
      const cur = state[action.id];
      return {
        ...state,
        [action.id]: {
          ...cur,
          floating: true,
          x: Math.round(action.x),
          y: Math.round(action.y),
          anchorRight: false,
        },
      };
    }
    case 'DOCK': {
      const def = createDefaultWindowLayout()[action.id];
      return {
        ...state,
        [action.id]: {
          ...state[action.id],
          floating: def.floating,
          x: def.x,
          y: def.y,
          anchorRight: def.anchorRight,
        },
      };
    }
    case 'CLOSE':
      return {
        ...state,
        [action.id]: { ...state[action.id], open: false },
      };
    case 'OPEN': {
      const top = maxZ(state);
      return {
        ...state,
        [action.id]: {
          ...state[action.id],
          open: true,
          minimized: false,
          zIndex: top + 1,
        },
      };
    }
    case 'TOGGLE_OPEN': {
      const cur = state[action.id];
      if (cur.open) {
        return {
          ...state,
          [action.id]: { ...cur, open: false },
        };
      }
      const top = maxZ(state);
      return {
        ...state,
        [action.id]: {
          ...cur,
          open: true,
          minimized: false,
          zIndex: top + 1,
        },
      };
    }
    case 'TOGGLE_MINIMIZE':
      return {
        ...state,
        [action.id]: {
          ...state[action.id],
          minimized: !state[action.id].minimized,
        },
      };
  }
}
