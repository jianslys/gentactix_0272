// GenTactix V2 frontend · 全局保姆级游戏上下文（消除 35 个 Props 传参地狱）
// 任何新写的 UI 窗口只要调 `const game = useGame();` 就能拿快照、发指令、控窗口、换美术。

import React, { useCallback, useContext, useEffect, useMemo, useReducer, useState } from 'react';
import { getLoadedArtCount, preloadArtAssets, reloadArtAssets, subscribeArtChanges } from '../art/assetLoader';
import { applyThemeTokensToRoot } from '../art/themeTokens';
import type { BackendAdapter } from '../engine/backendAdapter';
import { HttpEngineAdapter } from '../engine/httpEngineAdapter';
import { LocalMockSandboxAdapter } from '../engine/localMockSandbox';
import type {
  CommandOutcomeJson,
  IFieldTriplet,
  InitialWorld,
  TacticalCommandJson,
  Team,
  UnitDetailView,
  UnitState,
  WorldSnapshot,
} from '../engine/types';
import {
  clearWindowLayout,
  loadWindowLayout,
  saveWindowLayout,
  windowReducer,
  type ManagedWindowId,
  type WindowLayoutMap,
} from './windowStore';

export type BackendMode = 'mock' | 'http';

export interface BackendHealthInfo {
  ok: boolean;
  statusText: string;
  tick?: number;
}

export interface GameContextValue {
  backendMode: BackendMode;
  setBackendMode: (mode: BackendMode) => void;
  httpBaseUrl: string;
  setHttpBaseUrl: (url: string) => void;
  adapter: BackendAdapter;
  healthInfo: BackendHealthInfo | null;
  checkBackendHealth: () => Promise<void>;

  initialWorld: InitialWorld | null;
  snapshot: WorldSnapshot | null;
  displaySnapshot: WorldSnapshot | null;
  selectedUnit: UnitState | null;
  selectedUnitId: number | undefined;
  setSelectedUnitId: (id: number | undefined) => void;
  selectedUnitDetail: UnitDetailView | null;
  devSampleUnitDetail: UnitDetailView | null;
  setDevSampleUnitDetail: (detail: UnitDetailView | null) => void;

  iField: IFieldTriplet | null;
  refreshIField: () => Promise<void>;
  showThreatOverlay: boolean;
  setShowThreatOverlay: (v: boolean) => void;

  observer: Team;
  setObserver: (t: Team) => void;
  tickNumber: number;
  elapsedMs: number;
  playing: boolean;
  setPlaying: (v: boolean) => void;
  showUiIds: boolean;
  setShowUiIds: (v: boolean) => void;

  artVersion: number;
  loadedArtCount: number;
  reloadArt: () => void;

  windows: WindowLayoutMap;
  focusWindow: (id: ManagedWindowId) => void;
  moveWindow: (id: ManagedWindowId, x: number, y: number) => void;
  dockWindow: (id: ManagedWindowId) => void;
  closeWindow: (id: ManagedWindowId) => void;
  openWindow: (id: ManagedWindowId) => void;
  toggleWindow: (id: ManagedWindowId) => void;
  toggleMinimizeWindow: (id: ManagedWindowId) => void;
  resetWindows: () => void;

  stepNext: () => Promise<void>;
  jumpToTick: (tick: number) => Promise<void>;
  sendQuickCommand: (cmd: TacticalCommandJson, cmdSecInTick?: number) => Promise<CommandOutcomeJson | null>;
  triggerSampleShake: () => void;
  lastCommandOutcome: CommandOutcomeJson | null;
}

export const GameContext = React.createContext<GameContextValue | null>(null);

/** 读取全局游戏上下文；必须在 <GameProvider> 或 <App /> 内部调用。 */
export function useGame(): GameContextValue {
  const ctx = useContext(GameContext);
  if (!ctx) {
    throw new Error('useGame() 必须在 <GameProvider> 或 <App /> 内部使用');
  }
  return ctx;
}

/** 安全读取全局游戏上下文（用于被单独挂载测试的组件）。 */
export function useGameOptional(): GameContextValue | null {
  return useContext(GameContext);
}

/** Hook: 管理全部浮动窗口布局、持久化与重置。 */
export function useWindowManager() {
  const [windows, dispatch] = useReducer(windowReducer, undefined, loadWindowLayout);

  useEffect(() => {
    saveWindowLayout(windows);
  }, [windows]);

  const focusWindow = useCallback((id: ManagedWindowId) => dispatch({ type: 'FOCUS', id }), []);
  const moveWindow = useCallback((id: ManagedWindowId, x: number, y: number) => dispatch({ type: 'MOVE', id, x, y }), []);
  const dockWindow = useCallback((id: ManagedWindowId) => dispatch({ type: 'DOCK', id }), []);
  const closeWindow = useCallback((id: ManagedWindowId) => dispatch({ type: 'CLOSE', id }), []);
  const openWindow = useCallback((id: ManagedWindowId) => dispatch({ type: 'OPEN', id }), []);
  const toggleWindow = useCallback((id: ManagedWindowId) => dispatch({ type: 'TOGGLE_OPEN', id }), []);
  const toggleMinimizeWindow = useCallback(
    (id: ManagedWindowId) => dispatch({ type: 'TOGGLE_MINIMIZE', id }),
    [],
  );
  const resetWindows = useCallback(() => {
    clearWindowLayout();
    dispatch({ type: 'RESET' });
  }, []);

  return {
    windows,
    focusWindow,
    moveWindow,
    dockWindow,
    closeWindow,
    openWindow,
    toggleWindow,
    toggleMinimizeWindow,
    resetWindows,
  };
}

/** Hook: 注入主题 Token、预加载美术槽位并响应热重载。 */
export function useArtRegistry() {
  const [artVersion, setArtVersion] = useState(0);

  useEffect(() => {
    applyThemeTokensToRoot();
    preloadArtAssets();
    return subscribeArtChanges(() => setArtVersion((v) => v + 1));
  }, []);

  const reloadArt = useCallback(() => {
    reloadArtAssets();
  }, []);

  return {
    artVersion,
    loadedArtCount: getLoadedArtCount(),
    reloadArt,
  };
}

/** 独立测试/子应用专用保姆 Provider：默认挂 LocalMockSandboxAdapter，零配置可渲染任意业务面板。 */
export const GameProvider: React.FC<{
  adapter?: BackendAdapter;
  children: React.ReactNode;
}> = ({ adapter: injectedAdapter, children }) => {
  const [backendMode, setBackendMode] = useState<BackendMode>('mock');
  const [httpBaseUrl, setHttpBaseUrl] = useState('');
  const adapter = useMemo<BackendAdapter>(
    () =>
      injectedAdapter ??
      (backendMode === 'http' ? new HttpEngineAdapter({ baseUrl: httpBaseUrl }) : new LocalMockSandboxAdapter()),
    [injectedAdapter, backendMode, httpBaseUrl],
  );

  const [initialWorld, setInitialWorld] = useState<InitialWorld | null>(null);
  const [snapshot, setSnapshot] = useState<WorldSnapshot | null>(null);
  const [selectedUnitId, setSelectedUnitId] = useState<number | undefined>(undefined);
  const [selectedUnitDetail, setSelectedUnitDetail] = useState<UnitDetailView | null>(null);
  const [devSampleUnitDetail, setDevSampleUnitDetail] = useState<UnitDetailView | null>(null);
  const [iField, setIField] = useState<IFieldTriplet | null>(null);
  const [showThreatOverlay, setShowThreatOverlay] = useState(false);
  const [observer, setObserver] = useState<Team>('red');
  const [playing, setPlaying] = useState(false);
  const [showUiIds, setShowUiIds] = useState(true);
  const [healthInfo, setHealthInfo] = useState<BackendHealthInfo | null>(null);
  const [lastCommandOutcome, setLastCommandOutcome] = useState<CommandOutcomeJson | null>(null);

  const winMgr = useWindowManager();
  const art = useArtRegistry();

  useEffect(() => {
    let alive = true;
    void adapter.loadInitial(observer).then((init) => {
      if (!alive) return;
      setInitialWorld(init);
      setSnapshot(init.snapshot);
      if (init.snapshot.units.length > 0) {
        setSelectedUnitId((prev) => prev ?? init.snapshot.units[0]?.id);
      }
    });
    return () => {
      alive = false;
    };
  }, [adapter, observer]);

  useEffect(() => {
    if (!selectedUnitId || !adapter.getUnitDetail) {
      setSelectedUnitDetail(null);
      return;
    }
    let alive = true;
    void adapter.getUnitDetail(selectedUnitId, observer).then((d) => {
      if (alive) setSelectedUnitDetail(d ?? null);
    });
    return () => {
      alive = false;
    };
  }, [adapter, selectedUnitId, observer, snapshot?.tick]);

  const checkBackendHealth = useCallback(async () => {
    if (!adapter.checkHealth) {
      setHealthInfo({ ok: true, statusText: 'Mock 沙盒本地就绪', tick: snapshot?.tick });
      return;
    }
    const res = await adapter.checkHealth();
    setHealthInfo({
      ok: res.ok,
      statusText: res.ok
        ? `健康 v${res.version ?? '2.0'} (${res.cell_m ?? 8}m/${res.tick_s ?? 16}s)`
        : '离线/无法连接',
      tick: snapshot?.tick,
    });
  }, [adapter, snapshot?.tick]);

  const refreshIField = useCallback(async () => {
    if (!adapter.getIField) return;
    const f = await adapter.getIField(observer);
    setIField(f ?? null);
  }, [adapter, observer]);

  const stepNext = useCallback(async () => {
    const pb = await adapter.advanceTick(observer);
    setSnapshot(pb.final);
  }, [adapter, observer]);

  const jumpToTick = useCallback(
    async (tick: number) => {
      const list = await adapter.fastForwardTo(tick, observer);
      if (list.length > 0) {
        setSnapshot(list[list.length - 1]!.final);
      } else {
        const snap = await adapter.getSnapshot(observer);
        setSnapshot(snap);
      }
    },
    [adapter, observer],
  );

  const sendQuickCommand = useCallback(
    async (cmd: TacticalCommandJson, cmdSecInTick?: number) => {
      if (adapter.submitRawCommand) {
        const out = await adapter.submitRawCommand(cmd as Record<string, unknown>, cmdSecInTick);
        setLastCommandOutcome(out);
        const updated = await adapter.getSnapshot(observer);
        setSnapshot(updated);
        return out;
      }
      return null;
    },
    [adapter, observer],
  );

  const selectedUnit = useMemo(
    () => snapshot?.units.find((u) => u.id === selectedUnitId) ?? null,
    [snapshot, selectedUnitId],
  );

  const value: GameContextValue = {
    backendMode,
    setBackendMode,
    httpBaseUrl,
    setHttpBaseUrl,
    adapter,
    healthInfo,
    checkBackendHealth,
    initialWorld,
    snapshot,
    displaySnapshot: snapshot,
    selectedUnit,
    selectedUnitId,
    setSelectedUnitId,
    selectedUnitDetail,
    devSampleUnitDetail,
    setDevSampleUnitDetail,
    iField,
    refreshIField,
    showThreatOverlay,
    setShowThreatOverlay,
    observer,
    setObserver,
    tickNumber: snapshot?.tick ?? 0,
    elapsedMs: snapshot?.elapsedMs ?? 0,
    playing,
    setPlaying,
    showUiIds,
    setShowUiIds,
    ...art,
    ...winMgr,
    stepNext,
    jumpToTick,
    sendQuickCommand,
    triggerSampleShake: () => {},
    lastCommandOutcome,
  };

  return <GameContext.Provider value={value}>{children}</GameContext.Provider>;
};
