// GenTactix V2 frontend · 主装配（9 层渲染栈 + 16s 走表 + UIO 四区 + 独立指挥 UI + Layer 6 本地持久化）

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { LocalMockSandboxAdapter } from './engine/localMockSandbox';
import { playedLog, progressiveSnapshot } from './engine/playback';
import { DIRECT_COMMAND_RADIUS_M, HQ_UNIT_ID, RADIO_RADIUS_M, presetTerrain } from './engine/scenario';
import {
  TICK_MS,
  type DirectiveMode,
  type DirectiveTarget,
  type DirectiveTaskKind,
  type EngineerWorkKind,
  type FireDiscipline,
  type InitialWorld,
  type MoveStance,
  type ScheduledStepEvent,
  type TickPlayback,
  type UnitState,
  type Vec2,
  type WaveDirective,
  type WorldSnapshot,
} from './engine/types';
import { NineLayerViewport } from './render/NineLayerViewport';
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
  type UioState,
} from './store/uioStore';
import {
  addWaveMapPoint,
  commitPendingWaveShape,
  createEmptyWaveDraft,
  issueCommanderWave,
  previewWaveGeometry,
  type CommanderWaveDraft,
  type WaveDrawTool,
} from './store/waveStore';
import { CommanderWavePanel } from './ui/CommanderWavePanel';
import { FreehandToolbar } from './ui/FreehandToolbar';
import { TopControlBar } from './ui/TopControlBar';
import { UioClipboardPanel } from './ui/UioClipboardPanel';
import { UnitInspector } from './ui/UnitInspector';

export const App: React.FC = () => {
  const adapter = useMemo(() => new LocalMockSandboxAdapter(), []);
  const [initial, setInitial] = useState<InitialWorld | null>(null);
  const [baseSnapshot, setBaseSnapshot] = useState<WorldSnapshot | null>(null);
  const [waves, setWaves] = useState<WaveDirective[]>([]);
  const [activeWaveId, setActiveWaveId] = useState<string | undefined>(undefined);
  const [selectedUnitId, setSelectedUnitId] = useState<number | undefined>(HQ_UNIT_ID);

  // 16s 逐秒走表状态
  const [currentTick, setCurrentTick] = useState<number>(0);
  const [activePlayback, setActivePlayback] = useState<TickPlayback | null>(null);
  const [elapsedMs, setElapsedMs] = useState<number>(0);
  const [playing, setPlaying] = useState<boolean>(false);
  const [lookaheadEvents, setLookaheadEvents] = useState<ScheduledStepEvent[]>([]);
  const settledTicksRef = useRef<Set<number>>(new Set());

  // 手写板 UIO & 独立指挥 UI
  const [uio, setUio] = useState<UioState>(() => createInitialUioState());
  const [rightTab, setRightTab] = useState<'uio' | 'wave'>('uio');
  const [waveDraft, setWaveDraft] = useState<CommanderWaveDraft>(() => createEmptyWaveDraft());
  const [isPickingTarget, setIsPickingTarget] = useState<boolean>(false);
  const [mapPickTarget, setMapPickTarget] = useState<DirectiveTarget | undefined>(undefined);
  const [statusNote, setStatusNote] = useState<string>('');

  // Layer 6/7/9 本地态
  const [strokes, setStrokes] = useState<FreehandStroke[]>([]);
  const [freehandTool, setFreehandTool] = useState<FreehandTool>('none');
  const [freehandColor, setFreehandColor] = useState<string>('#dc2626');
  const [storageBackend, setStorageBackend] = useState<'indexeddb' | 'localstorage'>('indexeddb');
  const [l7Enabled, setL7Enabled] = useState<boolean>(true);
  const [l7Weather, setL7Weather] = useState<WeatherMode>('fog');
  const [l9Enabled, setL9Enabled] = useState<boolean>(true);

  const refreshLookahead = useCallback(
    async (tick: number) => {
      const evs = await adapter.lookahead(tick + 1, 'red');
      setLookaheadEvents(evs);
    },
    [adapter],
  );

  useEffect(() => {
    void adapter.loadInitial('red').then((w) => {
      setInitial(w);
      setBaseSnapshot(w.snapshot);
      setWaves(w.waves);
      setCurrentTick(w.snapshot.tick);
      void refreshLookahead(w.snapshot.tick);
    });
    void loadLayer6Strokes().then((saved) => {
      if (saved.length > 0) setStrokes(saved);
    });
  }, [adapter, refreshLookahead]);

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

  // 16s 实时逐秒播放循环（防剧透渐进演出）
  useEffect(() => {
    if (!playing || !activePlayback) return;
    const timer = window.setInterval(() => {
      setElapsedMs((prev) => {
        const next = Math.min(TICK_MS, prev + 250);
        if (next >= TICK_MS) {
          setPlaying(false);
          settlePlayback(activePlayback);
        }
        return next;
      });
    }, 250);
    return () => window.clearInterval(timer);
  }, [playing, activePlayback, settlePlayback]);

  const handleStepNext = async () => {
    if (activePlayback) settlePlayback(activePlayback);
    const pb = await adapter.advanceTick('red');
    setCurrentTick(pb.tick);
    setActivePlayback(pb);
    setBaseSnapshot(pb.final);
    setElapsedMs(0);
    setPlaying(true);
    await refreshLookahead(pb.tick);
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
    await refreshLookahead(targetTick);
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

  const handleMapClick = (worldPos: Vec2, hitUnit?: UnitState) => {
    if (rightTab === 'wave' && waveDraft.tool !== 'none') {
      setWaveDraft((d) => addWaveMapPoint(d, worldPos));
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
    await refreshLookahead(currentTick);
  };

  const handleIssueWave = async () => {
    try {
      const res = await issueCommanderWave(waveDraft, adapter);
      setWaves((prev) => [...prev, res.wave]);
      setWaveDraft(res.nextDraft);
      setActiveWaveId(res.wave.id);
      setRightTab('uio');
      setStatusNote(`已签发波次「${res.wave.title}」，G 场将在 #${res.wave.effectiveTick} 生效`);
      await refreshLookahead(currentTick);
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

  const activeWave = waves.find((w) => w.id === activeWaveId);
  const wavePreview = rightTab === 'wave' ? previewWaveGeometry(waveDraft) : undefined;
  const scheduledDetonations = lookaheadEvents
    .filter((ev) => ev.kind === 'detonation' && ev.targetPos)
    .map((ev) => ev.targetPos!);
  const shaking =
    activePlayback !== null &&
    activePlayback.shakeAtMs.some((ms) => Math.abs(elapsedMs - ms) < 400);

  return (
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
        scheduledDetonations={scheduledDetonations}
        strokes={strokes}
        freehandTool={freehandTool}
        freehandColor={freehandColor}
        onStrokesChange={handleStrokesChange}
        l7Enabled={l7Enabled}
        l7Weather={l7Weather}
        l9Enabled={l9Enabled}
        shaking={shaking}
        mapPickHint={isPickingTarget || (rightTab === 'wave' && waveDraft.tool !== 'none') ? 'pick' : undefined}
        onMapClick={handleMapClick}
        hudChildren={
          <div className="hud-shell">
            <div className="hud-top">
              <TopControlBar
                currentTick={currentTick}
                elapsedMs={elapsedMs}
                playing={playing}
                hasActivePlayback={activePlayback !== null}
                lookaheadEvents={lookaheadEvents}
                l7Enabled={l7Enabled}
                l7Weather={l7Weather}
                l9Enabled={l9Enabled}
                onStepNext={() => void handleStepNext()}
                onTogglePlay={() => setPlaying((p) => !p)}
                onScrubMs={handleScrubMs}
                onJumpToTick={(t) => void handleJumpToTick(t)}
                onToggleL7={() => setL7Enabled((v) => !v)}
                onChangeWeather={setL7Weather}
                onToggleL9={() => setL9Enabled((v) => !v)}
              />
            </div>

            <div className="hud-main">
              <div className="hud-left">
                <FreehandToolbar
                  tool={freehandTool}
                  color={freehandColor}
                  strokeCount={strokes.length}
                  storageBackend={storageBackend}
                  onChangeTool={setFreehandTool}
                  onChangeColor={setFreehandColor}
                  onClear={handleClearStrokes}
                />
                <UnitInspector
                  units={displaySnapshot.units}
                  hqUnitId={initial?.hqUnitId ?? HQ_UNIT_ID}
                  directRadiusM={initial?.directCommandRadiusM ?? DIRECT_COMMAND_RADIUS_M}
                  selectedUnitId={selectedUnitId}
                  log={activePlayback ? playedLog(activePlayback, elapsedMs) : []}
                  onSelectUnit={setSelectedUnitId}
                />
                {statusNote && (
                  <div className="panel meta-note" data-testid="status-note">
                    {statusNote}
                  </div>
                )}
              </div>

              <div className="hud-right">
                <div className="panel row" style={{ justifyContent: 'space-between' }}>
                  <div className="row">
                    <button
                      type="button"
                      data-testid="tab-uio"
                      className={rightTab === 'uio' ? 'btn active' : 'btn'}
                      onClick={() => setRightTab('uio')}
                    >
                      手写板 UIO
                    </button>
                    <button
                      type="button"
                      data-testid="tab-commander-wave"
                      className={rightTab === 'wave' ? 'btn active' : 'btn'}
                      onClick={() => setRightTab('wave')}
                    >
                      独立【指挥 UI】
                    </button>
                  </div>
                  <span className="meta-note">双模块物理分离</span>
                </div>

                {rightTab === 'uio' ? (
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
                    onToggleWave={(id) => setActiveWaveId((cur) => (cur === id ? undefined : id))}
                    onTogglePickTarget={(active) => setIsPickingTarget(active)}
                    onClearPickedTarget={() => setMapPickTarget(undefined)}
                    onCreateDraft={handleCreateDraft}
                    onBindDraftUnit={(id, unitId) => setUio((prev) => updateDraftDirective(prev, id, { unitId }))}
                    onBindDraftTarget={(id, target) => setUio((prev) => updateDraftDirective(prev, id, { target }))}
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
                    onClearDraft={() => setWaveDraft(createEmptyWaveDraft())}
                    onIssueWave={() => void handleIssueWave()}
                  />
                )}
              </div>
            </div>
          </div>
        }
      />
    </div>
  );
};
