// GenTactix V2 frontend · 单位实时检视器（随 1s..16s 走表防剧透渐进跳字）与演出日志

import React from 'react';
import { computeCommandDelay, remoteDelayText } from '../engine/directives';
import type { PlaybackLogEntry, UnitState } from '../engine/types';

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
      <div className="panel" data-testid="unit-inspector">
        <div className="panel-header">
          <span>单位态势检视（逐秒防剧透）</span>
          <span className="meta-note">{units.length} 可见</span>
        </div>

        <div className="row" style={{ marginBottom: '6px' }}>
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
            <div className="row" style={{ justifyContent: 'space-between' }}>
              <strong>{selected.name}</strong>
              <span data-testid="inspector-pos" className="meta-note">
                ({selected.pos.x.toFixed(1)}, {selected.pos.y.toFixed(1)}) m
              </span>
            </div>
            <div className="row" style={{ justifyContent: 'space-between' }}>
              <span data-testid="inspector-soldiers">
                人数：{selected.soldiers}/{selected.maxSoldiers}
              </span>
              <span data-testid="inspector-ammo">
                弹药：{selected.ammo}/{selected.maxAmmo}
              </span>
              <span data-testid="inspector-suppression">压制：{selected.suppression}%</span>
            </div>
            <div className="meta-note">
              姿态/动作：{selected.activity.type} · 进度 {Math.round(selected.workProgress * 100)}% · 纪律 {selected.discipline}
            </div>
            {selected.role === 'platoon_hq' && (
              <div className="meta-note" data-testid="hq-move-rule">
                排长指挥官仅能通过 UIO 手写板编写移动纸条执行移动。
              </div>
            )}
            {delay && delay.remote && <div className="remote-banner">{remoteDelayText(delay)}</div>}
          </div>
        )}
      </div>

      <div className="panel" data-testid="playback-log" style={{ maxHeight: '160px', overflowY: 'auto' }}>
        <div className="panel-header">
          <span>16s 演出日志</span>
        </div>
        {log.length === 0 ? (
          <div className="meta-note">推进时钟后按到达毫秒渐进播报</div>
        ) : (
          log.map((item, idx) => (
            <div key={`${item.ms}-${idx}`} className="meta-note">
              {item.text}
            </div>
          ))
        )}
      </div>
    </div>
  );
};
