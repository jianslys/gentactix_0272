// GenTactix V2 frontend · 单位态势检视器（W3: I1..I5 + W3.I6/I7）与 16s 演出日志（W4: G1）

import React from 'react';
import { computeCommandDelay, remoteDelayText } from '../engine/directives';
import type { PlaybackLogEntry, UnitState } from '../engine/types';
import { useGameOptional } from '../store/gameContext';
import { UiTag } from './UiTag';

export interface UnitInspectorBodyProps {
  units: UnitState[];
  hqUnitId: number;
  directRadiusM: number;
  selectedUnitId?: number;
  onSelectUnit: (id: number) => void;
}

const BODY_REGION_NAMES = ['头', '躯干', '上肢', '下肢'] as const;
const SUBSYSTEM_NAMES = ['引擎', '履带', '主炮', '座圈', '观瞄', '电台', '弹药', '油箱'] as const;

function integrityClass(v: number): string {
  if (v >= 0.7) return 'ok';
  if (v >= 0.3) return 'warn';
  return 'crit';
}

export const UnitInspectorBody: React.FC<UnitInspectorBodyProps> = ({
  units,
  hqUnitId,
  directRadiusM,
  selectedUnitId,
  onSelectUnit,
}) => {
  const game = useGameOptional();
  const hq = units.find((u) => u.id === hqUnitId);
  const selected = units.find((u) => u.id === selectedUnitId) ?? hq;
  const delay = selected ? computeCommandDelay(hq, selected, directRadiusM) : undefined;
  const detail = game?.devSampleUnitDetail ?? game?.selectedUnitDetail ?? null;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
      <div className="meta-note">{units.length} 可见</div>

      <div className="row" data-ui-id="I1">
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
            姿态/动作：{selected.activity.type} · 进度 {Math.round(selected.workProgress * 100)}% · 纪律{' '}
            {selected.discipline}
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

      {detail?.soldiers && detail.soldiers.length > 0 && (
        <div className="detail-card" data-testid="unit-detail-soldiers" data-ui-id="W3.I6">
          <div className="row" style={{ justifyContent: 'space-between' }}>
            <span className="meta-note">
              <UiTag id="W3.I6" />
              单兵 4 部位（{detail.callsign}）
            </span>
            {game?.devSampleUnitDetail && (
              <button
                type="button"
                className="win-btn"
                onClick={() => game.setDevSampleUnitDetail(null)}
              >
                退出样板
              </button>
            )}
          </div>
          {detail.soldiers.slice(0, 4).map((s) => (
            <div key={s.soldier_id} className="detail-soldier-row">
              <span className="meta-note">
                #{s.soldier_id} HP {Math.round(s.hp)}/{s.max_hp}
                {s.bleed_rate_per_tick > 0 ? ` · 出血${s.bleed_rate_per_tick.toFixed(1)}` : ''}
              </span>
              <div className="region-grid">
                {s.regions.map((r, rIdx) => (
                  <span key={rIdx} className={`region-pill ${integrityClass(r.integrity)}`}>
                    {BODY_REGION_NAMES[rIdx]} {Math.round(r.integrity * 100)}%
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {detail?.vehicle_damage && (
        <div className="detail-card" data-testid="unit-detail-vehicle" data-ui-id="W3.I7">
          <div className="row" style={{ justifyContent: 'space-between' }}>
            <span className="meta-note">
              <UiTag id="W3.I7" />
              装甲 8 子系统（{detail.callsign}）
            </span>
            {game?.devSampleUnitDetail && (
              <button
                type="button"
                className="win-btn"
                onClick={() => game.setDevSampleUnitDetail(null)}
              >
                退出样板
              </button>
            )}
          </div>
          <div className="meta-note">
            RHA 前/侧/后/顶：{detail.vehicle_damage.armor_rha.front_mm}/{detail.vehicle_damage.armor_rha.side_mm}/
            {detail.vehicle_damage.armor_rha.rear_mm}/{detail.vehicle_damage.armor_rha.top_mm}mm · 乘员{' '}
            {detail.vehicle_damage.crew_alive}/{detail.vehicle_damage.crew_max}
            {detail.vehicle_damage.burning ? ' · 🔥起火' : ''}
            {detail.vehicle_damage.ammo_cooked_off ? ' · 💥殉爆' : ''}
          </div>
          <div className="subsys-grid">
            {detail.vehicle_damage.subsystems.map((val, idx) => (
              <div key={idx} className={`region-pill ${integrityClass(val)}`}>
                {SUBSYSTEM_NAMES[idx]} {Math.round(val * 100)}%
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};

export const PlaybackLogBody: React.FC<{ log: PlaybackLogEntry[] }> = ({ log }) => (
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
);

export interface UnitInspectorProps {
  units: UnitState[];
  hqUnitId: number;
  directRadiusM: number;
  selectedUnitId?: number;
  log: PlaybackLogEntry[];
  onSelectUnit: (id: number) => void;
}

/** 独立兼容外壳：不包 FloatingWindow 时保留原有 W3/W4 渲染。 */
export const UnitInspector: React.FC<UnitInspectorProps> = ({
  units,
  hqUnitId,
  directRadiusM,
  selectedUnitId,
  log,
  onSelectUnit,
}) => (
  <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
    <div className="panel" data-testid="unit-inspector" data-ui-id="W3">
      <div className="panel-header">
        <span>
          <UiTag id="W3" kind="win" />
          单位态势检视（逐秒防剧透）
        </span>
      </div>
      <UnitInspectorBody
        units={units}
        hqUnitId={hqUnitId}
        directRadiusM={directRadiusM}
        selectedUnitId={selectedUnitId}
        onSelectUnit={onSelectUnit}
      />
    </div>
    <div className="panel" data-testid="playback-log" data-ui-id="W4" style={{ maxHeight: '160px', overflowY: 'auto' }}>
      <div className="panel-header">
        <span>
          <UiTag id="W4" kind="win" />
          16s 演出日志
        </span>
      </div>
      <PlaybackLogBody log={log} />
    </div>
  </div>
);
