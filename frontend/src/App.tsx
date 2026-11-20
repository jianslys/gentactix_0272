// GenTactix V2 frontend · 主装配（9 层全屏沙盘 + 16s 走表倍速 + UIO 四区 + 独立指挥 UI + UI ID 标牌）

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { BackendAdapter } from './engine/backendAdapter';
import { HttpEngineAdapter } from './engine/httpEngineAdapter';
import { LocalMockSandboxAdapter } from './engine/localMockSandbox';
import { playedLog, progressiveSnapshot } from './engine/playback';
import { DIRECT_COMMAND_RADIUS_M, HQ_UNIT_ID, RADIO_RADIUS_M, presetTerrain } from './engine/scenario';
import {
  TICK_MS,
  type CommandOutcomeJson,
  type DirectiveMode,
  type DirectiveTarget,
  type DirectiveTaskKind,
  type EngineerWorkKind,
  type FireDiscipline,
  type IFieldTriplet,
  type InitialWorld,
  type MoveStance,
  type ScheduledStepEvent,
  type TacticalCommandJson,
  type TickPlayback,
  type UnitDetailView,
  type UnitState,
  type Vec2,
  type WaveDirective,
  type WorldSnapshot,
} from './engine/types';
import { NineLayerViewport } from './render/NineLayerViewport';
import type { PlannedRouteOverlay } from './render/canvasLayers';
import {
  GameContext,
  useArtRegistry,
  useWindowManager,
  type BackendHealthInfo,
  type BackendMode,
  type GameContextValue,
} from './store/gameContext';
import {
  clearLayer6Strokes,
  loadLayer6Strokes,
  saveLayer6Strokes,
  type FreehandStroke,
  type FreehandTool,
  type WeatherMode,
} from './store/layerStore';
import {
  addDraftDirective,
  applyCompletedDirectives,
  copyDirectiveToDraft,
  createInitialUioState,
  issueTaskQueue,
  moveArchiveToQueue,
  moveDraftToQueue,
  moveQueueToDraft,
  removeDraftDirective,
  updateDraftDirective,
  updateQueueDirectiveTime,
  type UioState,
} from './store/uioStore';
import {
  addWaveCircle,
  addWaveMapPoint,
  commitPendingWaveShape,
  createEmptyWaveDraft,
  issueCommanderWave,
  previewWaveGeometry,
  undoWavePoint,
  type CommanderWaveDraft,
  type WaveDrawTool,
} from './store/waveStore';
import { CommanderWavePanel } from './ui/CommanderWavePanel';
import { DevArtPanel } from './ui/DevArtPanel';
import { ExampleCustomWindow } from './ui/ExampleCustomWindow';
import { FloatingWindow } from './ui/FloatingWindow';
import { FreehandToolbar } from './ui/FreehandToolbar';
import { TopControlBar, type PlaybackSpeed } from './ui/TopControlBar';
import { UiIdContext, UiTag } from './ui/UiTag';
import { UioClipboardPanel } from './ui/UioClipboardPanel';
import { PlaybackLogBody, UnitInspectorBody } from './ui/UnitInspector';

export const App: React.FC = () => {
  const [backendMode, setBackendModeState] = useState<BackendMode>('mock');
  const [httpBaseUrl, setHttpBaseUrl] = useState<string>('');
  const adapter = useMemo<BackendAdapter>(
    () =>
      backendMode === 'http' ? new HttpEngineAdapter({ baseUrl: httpBaseUrl }) : new LocalMockSandboxAdapter(),
    [backendMode, httpBaseUrl],
  );
  const [healthInfo, setHealthInfo] = useState<BackendHealthInfo | null>(null);
  const [selectedUnitDetail, setSelectedUnitDetail] = useState<UnitDetailView | null>(null);
  const [devSampleUnitDetail, setDevSampleUnitDetail] = useState<UnitDetailView | null>(null);
  const [iField, setIField] = useState<IFieldTriplet | null>(null);
  const [showThreatOverlay, setShowThreatOverlay] = useState<boolean>(false);
  const [sampleShake, setSampleShake] = useState<boolean>(false);
  const [lastCommandOutcome, setLastCommandOutcome] = useState<CommandOutcomeJson | null>(null);
  const winMgr = useWindowManager();
  const art = useArtRegistry();

  const [initial, setInitial] = useState<InitialWorld | null>(null);
  const [baseSnapshot, setBaseSnapshot] = useState<WorldSnapshot | null>(null);
  const [waves, setWaves] = useState<WaveDirective[]>([]);
  const [activeWaveId, setActiveWaveId] = useState<string | undefined>(undefined);
  const [selectedUnitId, setSelectedUnitId] = useState<number | undefined>(HQ_UNIT_ID);

  // 16s 逐秒走表与 60 天 StepCache 书签
  const [currentTick, setCurrentTick] = useState<number>(0);
  const [activePlayback, setActivePlayback] = useState<TickPlayback | null>(null);
  const [elapsedMs, setElapsedMs] = useState<number>(0);
  const [playing, setPlaying] = useState<boolean>(false);
  const [playbackSpeed, setPlaybackSpeed] = useState<PlaybackSpeed>(2);
  const [lookaheadEvents, setLookaheadEvents] = useState<ScheduledStepEvent[]>([]);
  const [scheduledEvents, setScheduledEvents] = useState<ScheduledStepEvent[]>([]);
  const settledTicksRef = useRef<Set<number>>(new Set());

  // 手写板 UIO & 独立指挥 UI & 地图联动状态
  const [uio, setUio] = useState<UioState>(() => createInitialUioState());
  const [rightTab, setRightTab] = useState<'uio' | 'wave'>('uio');
  const [rightCollapsed, setRightCollapsed] = useState<boolean>(false);
  const [waveDraft, setWaveDraft] = useState<CommanderWaveDraft>(() => createEmptyWaveDraft());
  const [isPickingTarget, setIsPickingTarget] = useState<boolean>(false);
  const [pickingDraftId, setPickingDraftId] = useState<string | undefined>(undefined);
  const [pickingMultiPoint, setPickingMultiPoint] = useState<boolean>(false);
  const [pickingPoints, setPickingPoints] = useState<Vec2[]>([]);
  const [mapPickTarget, setMapPickTarget] = useState<DirectiveTarget | undefined>(undefined);
  const [hoveredDirectiveId, setHoveredDirectiveId] = useState<string | undefined>(undefined);
  const [showPlannedRoutes, setShowPlannedRoutes] = useState<boolean>(true);
  const [cursorWorld, setCursorWorld] = useState<Vec2 | undefined>(undefined);
  const [statusNote, setStatusNote] = useState<string>('');

  // Layer 6/7/9 本地态 + UI ID 标牌可见开关
  const [strokes, setStrokes] = useState<FreehandStroke[]>([]);
  const [freehandTool, setFreehandTool] = useState<FreehandTool>('none');
  const [freehandColor, setFreehandColor] = useState<string>('#dc2626');
  const [storageBackend, setStorageBackend] = useState<'indexeddb' | 'localstorage'>('indexeddb');
  const [l7Enabled, setL7Enabled] = useState<boolean>(true);
  const [l7Weather, setL7Weather] = useState<WeatherMode>('fog');
  const [l9Enabled, setL9Enabled] = useState<boolean>(true);
  const [showUiIds, setShowUiIds] = useState<boolean>(true);

  const setBackendMode = useCallback((mode: BackendMode) => {
    setPlaying(false);
    setActivePlayback(null);
    setElapsedMs(0);
    settledTicksRef.current.clear();
    setBackendModeState(mode);
    setStatusNote(mode === 'http' ? '已切换至 HTTP 真后端 (:8767)' : '已切换至离线 Mock 沙盒');
  }, []);

  const refreshScheduleState = useCallback(
    async (tick: number) => {
      const [evs, all] = await Promise.all([adapter.lookahead(tick + 1, 'red'), adapter.listScheduled('red')]);
      setLookaheadEvents(evs);
      setScheduledEvents(all);
    },
    [adapter],
  );

  useEffect(() => {
    let alive = true;
    void adapter
      .loadInitial('red')
      .then((w) => {
        if (!alive) return;
        setInitial(w);
        setBaseSnapshot(w.snapshot);
        setWaves(w.waves);
        setCurrentTick(w.snapshot.tick);
        void refreshScheduleState(w.snapshot.tick);
      })
      .catch((err) => {
        if (!alive) return;
        setStatusNote(err instanceof Error ? `后端初始化失败：${err.message}` : '后端初始化失败');
      });
    void loadLayer6Strokes().then((saved) => {
      if (alive && saved.length > 0) setStrokes(saved);
    });
    return () => {
      alive = false;
    };
  }, [adapter, refreshScheduleState]);

  useEffect(() => {
    if (!selectedUnitId || !adapter.getUnitDetail) {
      setSelectedUnitDetail(null);
      return;
    }
    let alive = true;
    void adapter.getUnitDetail(selectedUnitId, 'red').then((d) => {
      if (alive) setSelectedUnitDetail(d ?? null);
    });
    return () => {
      alive = false;
    };
  }, [adapter, selectedUnitId, currentTick]);

  const checkBackendHealth = useCallback(async () => {
    if (!adapter.checkHealth) {
      setHealthInfo({ ok: true, statusText: 'Mock 沙盒本地就绪', tick: currentTick });
      return;
    }
    try {
      const res = await adapter.checkHealth();
      setHealthInfo({
        ok: res.ok,
        statusText: res.ok
          ? `健康 v${res.version ?? '2.0'} (${res.cell_m ?? 8}m/${res.tick_s ?? 16}s)`
          : '离线/无法连接',
        tick: currentTick,
      });
    } catch {
      setHealthInfo({ ok: false, statusText: 'HTTP 请求异常', tick: currentTick });
    }
  }, [adapter, currentTick]);

  const refreshIField = useCallback(async () => {
    if (!adapter.getIField) return;
    const f = await adapter.getIField('red');
    setIField(f ?? null);
  }, [adapter]);

  useEffect(() => {
    if (showThreatOverlay) {
      void refreshIField();
    }
  }, [showThreatOverlay, currentTick, refreshIField]);

  const triggerSampleShake = useCallback(() => {
    setSampleShake(true);
    window.setTimeout(() => setSampleShake(false), 420);
  }, []);

  const settlePlayback = useCallback((pb: TickPlayback) => {
    if (settledTicksRef.current.has(pb.tick)) return;
    settledTicksRef.current.add(pb.tick);
    if (pb.completedDirectiveIds.length > 0) {
      setUio((prev) => applyCompletedDirectives(prev, pb.completedDirectiveIds));
    }
    if (pb.effectiveWaveIds.length > 0) {
      const effSet = new Set(pb.effectiveWaveIds);
      setWaves((prev) => prev.map((w) => (effSet.has(w.id) ? { ...w, effective: true } : w)));
    }
  }, []);

  // 16s 实时逐秒播放循环（支持 1x / 2x / 4x 倍速，严禁提前剧透）
  useEffect(() => {
    if (!playing || !activePlayback) return;
    const stepMs = 200 * playbackSpeed;
    const timer = window.setInterval(() => {
      setElapsedMs((prev) => {
        const next = Math.min(TICK_MS, prev + stepMs);
        if (next >= TICK_MS) {
          setPlaying(false);
          settlePlayback(activePlayback);
        }
        return next;
      });
    }, 200);
    return () => window.clearInterval(timer);
  }, [playing, playbackSpeed, activePlayback, settlePlayback]);

  // 全局快捷键：Space 播停走表，Esc 退出当前标绘/拾取/手绘模式
  useEffect(() => {
    const onKeyDown = (ev: KeyboardEvent) => {
      const tag = (ev.target as HTMLElement | null)?.tagName;
      if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return;
      if (ev.code === 'Space' && activePlayback !== null) {
        ev.preventDefault();
        setPlaying((p) => !p);
      } else if (ev.code === 'Escape') {
        setIsPickingTarget(false);
        setPickingDraftId(undefined);
        setPickingMultiPoint(false);
        setPickingPoints([]);
        setFreehandTool('none');
        setWaveDraft((d) => ({ ...d, tool: 'none', pendingPoints: [] }));
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [activePlayback]);

  const handleStepNext = async () => {
    if (activePlayback) settlePlayback(activePlayback);
    const pb = await adapter.advanceTick('red');
    setCurrentTick(pb.tick);
    setActivePlayback(pb);
    setBaseSnapshot(pb.final);
    setElapsedMs(0);
    setPlaying(true);
    await refreshScheduleState(pb.tick);
  };

  const handleJumpToTick = async (targetTick: number) => {
    if (activePlayback) settlePlayback(activePlayback);
    setPlaying(false);
    const list = await adapter.fastForwardTo(targetTick, 'red');
    if (list.length > 0) {
      for (const pb of list) settlePlayback(pb);
      const last = list[list.length - 1]!;
      setActivePlayback(last);
      setBaseSnapshot(last.final);
      setCurrentTick(last.tick);
      setElapsedMs(TICK_MS);
    } else {
      const snap = await adapter.getSnapshot('red');
      setBaseSnapshot(snap);
      setCurrentTick(snap.tick);
    }
    await refreshScheduleState(targetTick);
  };

  const handleScrubMs = (ms: number) => {
    setPlaying(false);
    setElapsedMs(ms);
    if (activePlayback && ms >= TICK_MS) {
      settlePlayback(activePlayback);
    }
  };

  const handleStrokesChange = (next: FreehandStroke[]) => {
    setStrokes(next);
    void saveLayer6Strokes(next).then(setStorageBackend);
  };

  const handleClearStrokes = () => {
    setStrokes([]);
    void clearLayer6Strokes();
  };

  const finishPolylinePick = useCallback(() => {
    if (!pickingDraftId || pickingPoints.length === 0) {
      setPickingDraftId(undefined);
      setPickingMultiPoint(false);
      setPickingPoints([]);
      setIsPickingTarget(false);
      return;
    }
    const target: DirectiveTarget =
      pickingPoints.length === 1
        ? { type: 'point', pos: pickingPoints[0]! }
        : { type: 'polyline', points: pickingPoints };
    setUio((prev) => updateDraftDirective(prev, pickingDraftId, { target }));
    setMapPickTarget(target);
    setPickingDraftId(undefined);
    setPickingMultiPoint(false);
    setPickingPoints([]);
    setIsPickingTarget(false);
  }, [pickingDraftId, pickingPoints]);

  const handleMapClick = (worldPos: Vec2, hitUnit?: UnitState) => {
    if (rightTab === 'wave' && waveDraft.tool !== 'none') {
      if (waveDraft.tool !== 'barrage_circle') {
        setWaveDraft((d) => addWaveMapPoint(d, worldPos));
      }
      return;
    }
    if (pickingDraftId) {
      if (pickingMultiPoint) {
        setPickingPoints((pts) => [...pts, worldPos]);
        return;
      }
      const target: DirectiveTarget =
        hitUnit && hitUnit.team !== 'red'
          ? { type: 'unit', unitId: hitUnit.id }
          : { type: 'point', pos: worldPos };
      setUio((prev) => updateDraftDirective(prev, pickingDraftId, { target }));
      setMapPickTarget(target);
      setPickingDraftId(undefined);
      setIsPickingTarget(false);
      return;
    }
    if (isPickingTarget) {
      if (hitUnit && hitUnit.team !== 'red') {
        setMapPickTarget({ type: 'unit', unitId: hitUnit.id });
      } else {
        setMapPickTarget({ type: 'point', pos: worldPos });
      }
      setIsPickingTarget(false);
      return;
    }
    if (hitUnit) {
      setSelectedUnitId(hitUnit.id);
    }
  };

  const handleCreateDraft = (opts: {
    mode: DirectiveMode;
    taskKind: DirectiveTaskKind;
    unitId?: number;
    target?: DirectiveTarget;
    startSec: number;
    endSec: number;
    delayOffsetSec: number;
    delayMultiplier: number;
    stance: MoveStance;
    discipline: FireDiscipline;
    work: EngineerWorkKind;
  }) => {
    setUio((prev) => addDraftDirective(prev, opts).state);
  };

  const handleIssueQueue = async () => {
    const res = await issueTaskQueue(uio, adapter);
    setUio(res.state);
    setStatusNote(res.message);
    await refreshScheduleState(currentTick);
  };

  const handleIssueWave = async () => {
    try {
      const res = await issueCommanderWave(waveDraft, adapter);
      setWaves((prev) => [...prev, res.wave]);
      setWaveDraft(res.nextDraft);
      setActiveWaveId(res.wave.id);
      setRightTab('uio');
      setStatusNote(`已签发波次「${res.wave.title}」，G 场将在 #${res.wave.effectiveTick} 生效`);
      await refreshScheduleState(currentTick);
    } catch (err) {
      setStatusNote(err instanceof Error ? err.message : '波次签发失败');
    }
  };

  // 当前用于渲染的逐秒渐进快照（防剧透）
  const displaySnapshot: WorldSnapshot = useMemo(() => {
    if (activePlayback) {
      return progressiveSnapshot(activePlayback, elapsedMs);
    }
    return (
      baseSnapshot ?? {
        tick: 0,
        elapsedMs: 0,
        observer: 'red',
        units: [],
        groundObjects: [],
        destruction: [],
        smoke: [],
        night: false,
      }
    );
  }, [activePlayback, elapsedMs, baseSnapshot]);

  const plannedRoutes: PlannedRouteOverlay[] = useMemo(() => {
    const out: PlannedRouteOverlay[] = [];
    for (const d of uio.queue) {
      if (d.unitId !== undefined && d.target) {
        out.push({ id: d.id, unitId: d.unitId, target: d.target, locked: false, label: `#${d.seq} 待签` });
      }
    }
    for (const { directive: d } of uio.locked) {
      if (d.unitId !== undefined && d.target) {
        out.push({ id: d.id, unitId: d.unitId, target: d.target, locked: true, label: `#${d.seq} 执行中` });
      }
    }
    if (hoveredDirectiveId) {
      const hd = uio.drafts.find((d) => d.id === hoveredDirectiveId);
      if (hd && hd.unitId !== undefined && hd.target) {
        out.push({ id: hd.id, unitId: hd.unitId, target: hd.target, locked: false, label: `#${hd.seq} 草稿` });
      }
    }
    return out;
  }, [uio.queue, uio.locked, uio.drafts, hoveredDirectiveId]);

  const activeWave = waves.find((w) => w.id === activeWaveId);
  const wavePreview = rightTab === 'wave' ? previewWaveGeometry(waveDraft, cursorWorld) : undefined;
  const scheduledDetonations = lookaheadEvents
    .filter((ev) => ev.kind === 'detonation' && ev.targetPos)
    .map((ev) => ev.targetPos!);
  const sendQuickCommand = useCallback(
    async (cmd: TacticalCommandJson, cmdSecInTick?: number) => {
      if (!adapter.submitRawCommand) return null;
      const out = await adapter.submitRawCommand(cmd as Record<string, unknown>, cmdSecInTick);
      setLastCommandOutcome(out);
      const updated = await adapter.getSnapshot('red');
      setBaseSnapshot(updated);
      setCurrentTick(updated.tick);
      await refreshScheduleState(updated.tick);
      return out;
    },
    [adapter, refreshScheduleState],
  );

  const shaking =
    sampleShake ||
    (activePlayback !== null &&
      activePlayback.shakeAtMs.some((ms) => Math.abs(elapsedMs - ms) < 400));

  const mapPickHint = pickingDraftId
    ? pickingMultiPoint
      ? `正在为草稿折线拾取点位（已点 ${pickingPoints.length} 点，双击或右键完成，Esc 取消）`
      : '点击地图为草稿卡片绑定标的物（Esc 取消）'
    : isPickingTarget
      ? '点击地图拾取全局标的物（Esc 取消）'
      : rightTab === 'wave' && waveDraft.tool !== 'none'
        ? `独立指挥标绘：${waveDraft.tool}（Esc 退出）`
        : undefined;

  const selectedUnit = displaySnapshot.units.find((u) => u.id === selectedUnitId) ?? null;

  const gameContextValue: GameContextValue = {
    backendMode,
    setBackendMode,
    httpBaseUrl,
    setHttpBaseUrl,
    adapter,
    healthInfo,
    checkBackendHealth,
    initialWorld: initial,
    snapshot: baseSnapshot,
    displaySnapshot,
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
    observer: 'red',
    setObserver: () => {},
    tickNumber: currentTick,
    elapsedMs,
    playing,
    setPlaying,
    showUiIds,
    setShowUiIds,
    ...art,
    ...winMgr,
    stepNext: handleStepNext,
    jumpToTick: handleJumpToTick,
    sendQuickCommand,
    triggerSampleShake,
    lastCommandOutcome,
  };

  return (
    <GameContext.Provider value={gameContextValue}>
      <UiIdContext.Provider value={showUiIds}>
        <div onContextMenu={(e) => e.preventDefault()} style={{ width: '100%', height: '100%' }}>
          <NineLayerViewport
            terrain={initial?.terrain ?? presetTerrain()}
            snapshot={displaySnapshot}
            hqUnitId={initial?.hqUnitId ?? HQ_UNIT_ID}
            directRadiusM={initial?.directCommandRadiusM ?? DIRECT_COMMAND_RADIUS_M}
            radioRadiusM={initial?.radioRadiusM ?? RADIO_RADIUS_M}
            selectedUnitId={selectedUnitId}
            activeWave={activeWave}
            waveDraftGeometry={wavePreview}
            draftTarget={mapPickTarget}
            plannedRoutes={plannedRoutes}
            hoveredDirectiveId={hoveredDirectiveId}
            showPlannedRoutes={showPlannedRoutes}
            pickingPoints={pickingPoints}
            circleDragMode={rightTab === 'wave' && waveDraft.tool === 'barrage_circle'}
            onCircleDraw={(center, radiusM) => setWaveDraft((d) => addWaveCircle(d, center, radiusM))}
            onFinishPolyline={finishPolylinePick}
            onCursorWorldMove={setCursorWorld}
            activePlayback={activePlayback}
            elapsedMs={elapsedMs}
            scheduledDetonations={scheduledDetonations}
            strokes={strokes}
            freehandTool={freehandTool}
            freehandColor={freehandColor}
            onStrokesChange={handleStrokesChange}
            l7Enabled={l7Enabled}
            l7Weather={l7Weather}
            l9Enabled={l9Enabled}
            shaking={shaking}
            rightInsetPx={winMgr.windows.W5.open && !winMgr.windows.W5.floating ? (rightCollapsed ? 140 : 456) : 24}
            iField={iField}
            showThreatOverlay={showThreatOverlay}
            artVersion={art.artVersion}
            mapPickHint={mapPickHint}
            onMapClick={handleMapClick}
            hudChildren={
              <div className="hud-shell">
                <div className="hud-top">
                  <TopControlBar
                    currentTick={currentTick}
                    elapsedMs={elapsedMs}
                    playing={playing}
                    playbackSpeed={playbackSpeed}
                    hasActivePlayback={activePlayback !== null}
                    lookaheadEvents={lookaheadEvents}
                    scheduledEvents={scheduledEvents}
                    l7Enabled={l7Enabled}
                    l7Weather={l7Weather}
                    l9Enabled={l9Enabled}
                    showUiIds={showUiIds}
                    rightCollapsed={rightCollapsed}
                    onStepNext={() => void handleStepNext()}
                    onTogglePlay={() => setPlaying((p) => !p)}
                    onChangeSpeed={setPlaybackSpeed}
                    onScrubMs={handleScrubMs}
                    onJumpToTick={(t) => void handleJumpToTick(t)}
                    onToggleL7={() => setL7Enabled((v) => !v)}
                    onChangeWeather={setL7Weather}
                    onToggleL9={() => setL9Enabled((v) => !v)}
                    onToggleUiIds={() => setShowUiIds((v) => !v)}
                    onToggleRightCollapsed={() => setRightCollapsed((v) => !v)}
                  />
                </div>

                <div className="hud-main">
                  <div className="hud-left">
                    <FloatingWindow
                      id="W2"
                      testId="freehand-toolbar"
                      winState={winMgr.windows.W2}
                      onFocus={winMgr.focusWindow}
                      onMove={winMgr.moveWindow}
                      onToggleMinimize={winMgr.toggleMinimizeWindow}
                      onClose={winMgr.closeWindow}
                    >
                      <FreehandToolbar
                        embedded
                        tool={freehandTool}
                        color={freehandColor}
                        strokeCount={strokes.length}
                        storageBackend={storageBackend}
                        onChangeTool={setFreehandTool}
                        onChangeColor={setFreehandColor}
                        onClear={handleClearStrokes}
                      />
                    </FloatingWindow>
                    <FloatingWindow
                      id="W3"
                      testId="unit-inspector"
                      winState={winMgr.windows.W3}
                      onFocus={winMgr.focusWindow}
                      onMove={winMgr.moveWindow}
                      onToggleMinimize={winMgr.toggleMinimizeWindow}
                      onClose={winMgr.closeWindow}
                    >
                      <UnitInspectorBody
                        units={displaySnapshot.units}
                        hqUnitId={initial?.hqUnitId ?? HQ_UNIT_ID}
                        directRadiusM={initial?.directCommandRadiusM ?? DIRECT_COMMAND_RADIUS_M}
                        selectedUnitId={selectedUnitId}
                        onSelectUnit={setSelectedUnitId}
                      />
                    </FloatingWindow>
                    <FloatingWindow
                      id="W4"
                      testId="playback-log"
                      winState={winMgr.windows.W4}
                      onFocus={winMgr.focusWindow}
                      onMove={winMgr.moveWindow}
                      onToggleMinimize={winMgr.toggleMinimizeWindow}
                      onClose={winMgr.closeWindow}
                    >
                      <PlaybackLogBody log={activePlayback ? playedLog(activePlayback, elapsedMs) : []} />
                    </FloatingWindow>
                    {statusNote && (
                      <div className="panel meta-note" data-testid="status-note">
                        {statusNote}
                      </div>
                    )}
                  </div>

                  <FloatingWindow
                    id="W5"
                    testId="right-drawer"
                    className={rightCollapsed ? 'hud-right collapsed' : 'hud-right'}
                    winState={winMgr.windows.W5}
                    onFocus={winMgr.focusWindow}
                    onMove={winMgr.moveWindow}
                    onToggleMinimize={winMgr.toggleMinimizeWindow}
                    onClose={winMgr.closeWindow}
                  >
                    <div
                      className="row"
                      style={{ justifyContent: 'space-between', marginBottom: rightCollapsed ? 0 : '6px' }}
                    >
                      <div className="row">
                        {!rightCollapsed && (
                          <>
                            <button
                              type="button"
                              data-testid="tab-uio"
                              data-ui-id="R1"
                              className={rightTab === 'uio' ? 'btn active' : 'btn'}
                              onClick={() => setRightTab('uio')}
                            >
                              <UiTag id="R1" />
                              手写板 UIO
                            </button>
                            <button
                              type="button"
                              data-testid="tab-commander-wave"
                              data-ui-id="R2"
                              className={rightTab === 'wave' ? 'btn active' : 'btn'}
                              onClick={() => setRightTab('wave')}
                            >
                              <UiTag id="R2" />
                              独立【指挥 UI】
                            </button>
                          </>
                        )}
                      </div>
                      <button
                        type="button"
                        data-testid="btn-collapse-right"
                        data-ui-id="R3"
                        className="btn"
                        onClick={() => setRightCollapsed((v) => !v)}
                      >
                        <UiTag id="R3" />
                        {rightCollapsed ? '展开手写板' : '收起'}
                      </button>
                    </div>

                    {!rightCollapsed &&
                      (rightTab === 'uio' ? (
                        <UioClipboardPanel
                          uio={uio}
                          units={displaySnapshot.units}
                          hqUnitId={initial?.hqUnitId ?? HQ_UNIT_ID}
                          directRadiusM={initial?.directCommandRadiusM ?? DIRECT_COMMAND_RADIUS_M}
                          waves={waves}
                          activeWaveId={activeWaveId}
                          selectedUnitId={selectedUnitId}
                          mapPickTarget={mapPickTarget}
                          isPickingTarget={isPickingTarget}
                          pickingDraftId={pickingDraftId}
                          pickingPointsCount={pickingPoints.length}
                          hoveredDirectiveId={hoveredDirectiveId}
                          showPlannedRoutes={showPlannedRoutes}
                          onTogglePlannedRoutes={() => setShowPlannedRoutes((v) => !v)}
                          onHoverDirective={setHoveredDirectiveId}
                          onToggleWave={(id) => setActiveWaveId((cur) => (cur === id ? undefined : id))}
                          onTogglePickTarget={(active) => {
                            setPickingDraftId(undefined);
                            setPickingMultiPoint(false);
                            setPickingPoints([]);
                            setIsPickingTarget(active);
                          }}
                          onStartPickForDraft={(draftId, multiPoint) => {
                            setIsPickingTarget(false);
                            setPickingDraftId(draftId);
                            setPickingMultiPoint(multiPoint);
                            setPickingPoints([]);
                          }}
                          onFinishPolylinePick={finishPolylinePick}
                          onClearPickedTarget={() => setMapPickTarget(undefined)}
                          onCreateDraft={handleCreateDraft}
                          onBindDraftUnit={(id, unitId) => setUio((prev) => updateDraftDirective(prev, id, { unitId }))}
                          onBindDraftTarget={(id, target) => setUio((prev) => updateDraftDirective(prev, id, { target }))}
                          onUpdateDraftTime={(id, s, e) => setUio((prev) => updateDraftDirective(prev, id, { startSec: s, endSec: e }))}
                          onUpdateQueueTime={(id, s, e) => setUio((prev) => updateQueueDirectiveTime(prev, id, s, e))}
                          onMoveToQueue={(id) => setUio((prev) => moveDraftToQueue(prev, id).state)}
                          onMoveToDraft={(id) => setUio((prev) => moveQueueToDraft(prev, id))}
                          onDeleteDraft={(id) => setUio((prev) => removeDraftDirective(prev, id))}
                          onCopyToDraft={(id) => setUio((prev) => copyDirectiveToDraft(prev, id))}
                          onRequeueArchive={(id) => setUio((prev) => moveArchiveToQueue(prev, id).state)}
                          onIssueQueue={() => void handleIssueQueue()}
                        />
                      ) : (
                        <CommanderWavePanel
                          draft={waveDraft}
                          onChangeTitle={(title) => setWaveDraft((d) => ({ ...d, title }))}
                          onSelectTool={(tool: WaveDrawTool) => setWaveDraft((d) => ({ ...d, tool, pendingPoints: [] }))}
                          onCommitShape={() => setWaveDraft((d) => commitPendingWaveShape(d))}
                          onUndoPoint={() => setWaveDraft((d) => undoWavePoint(d))}
                          onClearDraft={() => setWaveDraft(createEmptyWaveDraft())}
                          onIssueWave={() => void handleIssueWave()}
                        />
                      ))}
                  </FloatingWindow>
                </div>

                <FloatingWindow
                  id="W7"
                  winState={winMgr.windows.W7}
                  onFocus={winMgr.focusWindow}
                  onMove={winMgr.moveWindow}
                  onToggleMinimize={winMgr.toggleMinimizeWindow}
                  onClose={winMgr.closeWindow}
                >
                  <DevArtPanel />
                </FloatingWindow>
                <FloatingWindow
                  id="W8"
                  winState={winMgr.windows.W8}
                  onFocus={winMgr.focusWindow}
                  onMove={winMgr.moveWindow}
                  onToggleMinimize={winMgr.toggleMinimizeWindow}
                  onClose={winMgr.closeWindow}
                >
                  <ExampleCustomWindow />
                </FloatingWindow>
              </div>
            }
          />
        </div>
      </UiIdContext.Provider>
    </GameContext.Provider>
  );
};
