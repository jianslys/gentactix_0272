// GenTactix V2 frontend · 单位态势检视器（W3: I1..I5）与 16s 演出日志（W4: G1）

import React from 'react';
import { computeCommandDelay, remoteDelayText } from '../engine/directives';
import type { PlaybackLogEntry, UnitState } from '../engine/types';
import { UiTag } from './UiTag';

export interface UnitInspectorProps {
  units: UnitState[];
  hqUnitId: number;
  directRadiusM: number;
  selectedUnitId?: number;
  log: PlaybackLogEntry[];
  onSelectUnit: (id: number) => void;
}

export const UnitInspector: React.FC<UnitInspectorProps> = ({
  units,
  hqUnitId,
  directRadiusM,
  selectedUnitId,
  log,
  onSelectUnit,
}) => {
  const hq = units.find((u) => u.id === hqUnitId);
  const selected = units.find((u) => u.id === selectedUnitId) ?? hq;
  const delay = selected ? computeCommandDelay(hq, selected, directRadiusM) : undefined;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
      <div className="panel" data-testid="unit-inspector" data-ui-id="W3">
        <div className="panel-header">
          <span>
            <UiTag id="W3" kind="win" />
            单位态势检视（逐秒防剧透）
          </span>
          <span className="meta-note">{units.length} 可见</span>
        </div>

        <div className="row" style={{ marginBottom: '6px' }} data-ui-id="I1">
          <UiTag id="I1" />
          {units.map((u) => (
            <button
              key={u.id}
              type="button"
              data-testid={`select-unit-${u.id}`}
              className={u.id === selected?.id ? 'btn active' : 'btn'}
              onClick={() => onSelectUnit(u.id)}
            >
              {u.name}
            </button>
          ))}
        </div>

        {selected && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
            <div className="row" style={{ justifyContent: 'space-between' }} data-ui-id="I2">
              <strong>
                <UiTag id="I2" />
                {selected.name}
              </strong>
              <span data-testid="inspector-pos" className="meta-note">
                ({selected.pos.x.toFixed(1)}, {selected.pos.y.toFixed(1)}) m
              </span>
            </div>
            <div className="row" style={{ justifyContent: 'space-between' }} data-ui-id="I3">
              <span data-testid="inspector-soldiers">
                <UiTag id="I3" />
                人数：{selected.soldiers}/{selected.maxSoldiers}
              </span>
              <span data-testid="inspector-ammo">
                弹药：{selected.ammo}/{selected.maxAmmo}
              </span>
              <span data-testid="inspector-suppression">压制：{selected.suppression}%</span>
            </div>
            <div className="meta-note" data-ui-id="I4">
              <UiTag id="I4" />
              姿态/动作：{selected.activity.type} · 进度 {Math.round(selected.workProgress * 100)}% · 纪律 {selected.discipline}
            </div>
            {selected.role === 'platoon_hq' && (
              <div className="meta-note" data-testid="hq-move-rule" data-ui-id="I5">
                <UiTag id="I5" />
                排长指挥官仅能通过 UIO 手写板编写移动纸条执行移动。
              </div>
            )}
            {delay && delay.remote && (
              <div className="remote-banner" data-ui-id="I5">
                <UiTag id="I5" />
                {remoteDelayText(delay)}
              </div>
            )}
          </div>
        )}
      </div>

      <div
        className="panel"
        data-testid="playback-log"
        data-ui-id="W4"
        style={{ maxHeight: '160px', overflowY: 'auto' }}
      >
        <div className="panel-header">
          <span>
            <UiTag id="W4" kind="win" />
            16s 演出日志
          </span>
        </div>
        <div data-ui-id="G1">
          <UiTag id="G1" />
          {log.length === 0 ? (
            <span className="meta-note">推进时钟后按到达毫秒渐进播报</span>
          ) : (
            log.map((item, idx) => (
              <div key={`${item.ms}-${idx}`} className="meta-note">
                {item.text}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
};
