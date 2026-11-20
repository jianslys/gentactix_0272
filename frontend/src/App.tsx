// GenTactix V2 frontend · Phase 2 初始装配壳（Phase 3 将接入 UIO 四区与指挥 UI）

import React, { useEffect, useMemo, useState } from 'react';
import { LocalMockSandboxAdapter } from './engine/localMockSandbox';
import { DIRECT_COMMAND_RADIUS_M, HQ_UNIT_ID, RADIO_RADIUS_M, presetTerrain } from './engine/scenario';
import type { InitialWorld, UnitState, Vec2, WorldSnapshot } from './engine/types';
import { NineLayerViewport } from './render/NineLayerViewport';
import {
  clearLayer6Strokes,
  loadLayer6Strokes,
  saveLayer6Strokes,
  type FreehandStroke,
  type FreehandTool,
  type WeatherMode,
} from './store/layerStore';
import { FreehandToolbar } from './ui/FreehandToolbar';

export const App: React.FC = () => {
  const adapter = useMemo(() => new LocalMockSandboxAdapter(), []);
  const [initial, setInitial] = useState<InitialWorld | null>(null);
  const [snapshot, setSnapshot] = useState<WorldSnapshot | null>(null);
  const [selectedUnitId, setSelectedUnitId] = useState<number | undefined>(HQ_UNIT_ID);

  const [strokes, setStrokes] = useState<FreehandStroke[]>([]);
  const [freehandTool, setFreehandTool] = useState<FreehandTool>('none');
  const [freehandColor, setFreehandColor] = useState<string>('#dc2626');
  const [storageBackend, setStorageBackend] = useState<'indexeddb' | 'localstorage'>('indexeddb');
  const [l7Enabled, setL7Enabled] = useState<boolean>(true);
  const [l7Weather, setL7Weather] = useState<WeatherMode>('fog');
  const [l9Enabled, setL9Enabled] = useState<boolean>(true);

  useEffect(() => {
    void adapter.loadInitial('red').then((w) => {
      setInitial(w);
      setSnapshot(w.snapshot);
    });
    void loadLayer6Strokes().then((saved) => {
      if (saved.length > 0) setStrokes(saved);
    });
  }, [adapter]);

  const handleStrokesChange = (next: FreehandStroke[]) => {
    setStrokes(next);
    void saveLayer6Strokes(next).then(setStorageBackend);
  };

  const handleClearStrokes = () => {
    setStrokes([]);
    void clearLayer6Strokes();
  };

  const handleMapClick = (_pos: Vec2, hitUnit?: UnitState) => {
    if (hitUnit) setSelectedUnitId(hitUnit.id);
  };

  const terrain = initial?.terrain ?? presetTerrain();
  const snap: WorldSnapshot = snapshot ?? {
    tick: 0,
    elapsedMs: 0,
    observer: 'red',
    units: [],
    groundObjects: [],
    destruction: [],
    smoke: [],
    night: false,
  };

  return (
    <NineLayerViewport
      terrain={terrain}
      snapshot={snap}
      hqUnitId={initial?.hqUnitId ?? HQ_UNIT_ID}
      directRadiusM={initial?.directCommandRadiusM ?? DIRECT_COMMAND_RADIUS_M}
      radioRadiusM={initial?.radioRadiusM ?? RADIO_RADIUS_M}
      selectedUnitId={selectedUnitId}
      strokes={strokes}
      freehandTool={freehandTool}
      freehandColor={freehandColor}
      onStrokesChange={handleStrokesChange}
      l7Enabled={l7Enabled}
      l7Weather={l7Weather}
      l9Enabled={l9Enabled}
      onMapClick={handleMapClick}
      hudChildren={
        <div className="hud-shell">
          <div className="hud-top">
            <div className="panel row">
              <button
                type="button"
                data-testid="toggle-l7"
                className={l7Enabled ? 'btn active' : 'btn'}
                onClick={() => setL7Enabled((v) => !v)}
              >
                L7 特效A：{l7Enabled ? '开' : '关'}
              </button>
              <select
                className="select"
                value={l7Weather}
                onChange={(e) => setL7Weather(e.target.value as WeatherMode)}
              >
                <option value="fog">战场薄雾</option>
                <option value="rain">细雨</option>
                <option value="snow">飘雪</option>
              </select>
              <button
                type="button"
                data-testid="toggle-l9"
                className={l9Enabled ? 'btn active' : 'btn'}
                onClick={() => setL9Enabled((v) => !v)}
              >
                L9 气氛B：{l9Enabled ? '开' : '关'}
              </button>
            </div>
            <FreehandToolbar
              tool={freehandTool}
              color={freehandColor}
              strokeCount={strokes.length}
              storageBackend={storageBackend}
              onChangeTool={setFreehandTool}
              onChangeColor={setFreehandColor}
              onClear={handleClearStrokes}
            />
          </div>
        </div>
      }
    />
  );
};
