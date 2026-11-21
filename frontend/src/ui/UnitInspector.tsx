// GenTactix V2 frontend · 单位编制卡片（W3: 计算机基础 UI，点兵牌弹出 + 单兵详情与物品栏）与兼容存根

import React, { useEffect, useState } from 'react';
import { computeCommandDelay, remoteDelayText } from '../engine/directives';
import type { PlaybackLogEntry, SoldierDetailRecord, UnitDetailView, UnitState } from '../engine/types';
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

const ROLE_LABELS: Record<string, string> = {
  platoon_hq: '排部指挥组',
  rifle_squad: '步兵班',
  enemy_squad: '敌步兵班',
  infantry_squad: '步兵班',
  heavy_weapon_team: '重火力组',
  engineer_squad: '工兵班',
  armored_vehicle: '装甲车组',
  recon_team: '侦察组',
};

const INV_CATEGORY_LABEL: Record<string, string> = {
  weapon: '武器',
  ammo: '弹药',
  medical: '医疗',
  gear: '装具',
};

function integrityClass(v: number): string {
  if (v >= 0.7) return 'ok';
  if (v >= 0.3) return 'warn';
  return 'crit';
}

export interface UnitCardModalProps {
  unit: UnitState;
  hq?: UnitState;
  directRadiusM: number;
  detail: UnitDetailView | null;
  onClose: () => void;
}

/** 点击地图任一兵牌弹出的基础 UI 单位编制卡片（W3）：组织度、兵力人数、单兵花名册、单兵情况与个人物品栏 */
export const UnitCardModal: React.FC<UnitCardModalProps> = ({
  unit,
  hq,
  directRadiusM,
  detail,
  onClose,
}) => {
  const [selectedSoldierId, setSelectedSoldierId] = useState<number | null>(null);
  const [dragPos, setDragPos] = useState<{ x: number; y: number }>({ x: 24, y: 24 });
  const [dragging, setDragging] = useState<{ sx: number; sy: number; ox: number; oy: number } | null>(null);

  useEffect(() => {
    setSelectedSoldierId(null);
  }, [unit.id]);

  useEffect(() => {
    if (!dragging) return;
    const onMove = (ev: PointerEvent) => {
      const vw = typeof window !== 'undefined' && window.innerWidth > 0 ? window.innerWidth : 1280;
      const vh = typeof window !== 'undefined' && window.innerHeight > 0 ? window.innerHeight : 800;
      const nx = Math.max(8, Math.min(vw - 440, dragging.ox + (ev.clientX - dragging.sx)));
      const ny = Math.max(8, Math.min(vh - 120, dragging.oy + (ev.clientY - dragging.sy)));
      setDragPos({ x: nx, y: ny });
    };
    const onUp = () => setDragging(null);
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    return () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
    };
  }, [dragging]);

  const delay = hq ? computeCommandDelay(hq, unit, directRadiusM) : undefined;
  const organizationPct = Math.max(0, 100 - (unit.suppression ?? 0));
  const soldiersList: SoldierDetailRecord[] =
    detail?.soldiers && detail.soldiers.length > 0
      ? detail.soldiers
      : Array.from({ length: unit.maxSoldiers }, (_, idx) => ({
          soldier_id: idx + 1,
          name: `士兵 #${idx + 1}`,
          roleLabel: idx === 0 ? '班长' : '步枪兵',
          alive: idx < unit.soldiers,
          hp: idx < unit.soldiers ? 11.1 : 0,
          max_hp: 11.1,
          regions: [
            { integrity: idx < unit.soldiers ? 1 : 0, cumulative_damage: 0 },
            { integrity: idx < unit.soldiers ? 1 : 0, cumulative_damage: 0 },
            { integrity: idx < unit.soldiers ? 1 : 0, cumulative_damage: 0 },
            { integrity: idx < unit.soldiers ? 1 : 0, cumulative_damage: 0 },
          ],
          bleeding_loss: 0,
          bleed_rate_per_tick: 0,
          suppression: unit.suppression / 10,
          inventory: [],
        }));

  const activeSoldier = soldiersList.find((s) => s.soldier_id === selectedSoldierId) ?? null;

  return (
    <section
      className="basic-ui-win unit-card-modal"
      data-testid="unit-card-modal"
      data-ui-id="W3"
      style={{ left: `${dragPos.x}px`, top: `${dragPos.y}px` }}
    >
      {/* 标题栏 */}
      <header
        className="basic-ui-header"
        data-ui-id="W3.HDR"
        onPointerDown={(e) => {
          if ((e.target as HTMLElement).closest('button')) return;
          setDragging({ sx: e.clientX, sy: e.clientY, ox: dragPos.x, oy: dragPos.y });
        }}
      >
        <div className="basic-ui-title">
          <UiTag id="W3" kind="win" source="src/ui/UnitInspector.tsx" />
          <span>
            单位编制卡片 · {unit.name} ({unit.team === 'red' ? '己方' : '敌方'})
          </span>
        </div>
        <button
          type="button"
          className="basic-ui-btn"
          data-testid="unit-card-close"
          data-ui-id="W3.CLOSE"
          onClick={onClose}
        >
          <UiTag id="W3.CLOSE" />×
        </button>
      </header>

      <div className="basic-ui-body">
        {/* 一、单位编制核心指标 */}
        <div className="basic-ui-section" data-ui-id="W3.ORG">
          <div className="basic-ui-section-title">
            <UiTag id="W3.ORG" />
            <strong>单位编制与状态</strong>
          </div>
          <table className="basic-ui-table">
            <tbody>
              <tr data-ui-id="W3.ORG.ROLE">
                <th>
                  <UiTag id="W3.ORG.ROLE" /> 编制体制
                </th>
                <td>
                  {ROLE_LABELS[unit.role] ?? unit.role} · ID #{unit.id}
                </td>
                <th>
                  <UiTag id="W3.ORG.POS" /> 坐标位置
                </th>
                <td>
                  ({Math.round(unit.pos.x)}, {Math.round(unit.pos.y)}) m
                </td>
              </tr>
              <tr data-ui-id="W3.ORG.MEN">
                <th>
                  <UiTag id="W3.ORG.MEN" /> 兵力人数
                </th>
                <td data-testid="unit-card-strength">
                  <strong>
                    {unit.soldiers} / {unit.maxSoldiers}
                  </strong>{' '}
                  人
                </td>
                <th>
                  <UiTag id="W3.ORG.MORALE" /> 组织度
                </th>
                <td data-testid="unit-card-org">
                  <strong>{organizationPct}%</strong> (压制 {unit.suppression}%)
                </td>
              </tr>
              <tr data-ui-id="W3.ORG.AMMO">
                <th>
                  <UiTag id="W3.ORG.AMMO" /> 弹药储备
                </th>
                <td>
                  {unit.ammo} / {unit.maxAmmo} 发
                </td>
                <th>
                  <UiTag id="W3.ORG.CMD" /> 通信通联
                </th>
                <td>{delay?.remote ? remoteDelayText(delay) : '直通 (0s)'}</td>
              </tr>
            </tbody>
          </table>
        </div>

        {/* 二、编制内士兵花名册（点选任一士兵展开详情与物品栏） */}
        <div className="basic-ui-section" data-ui-id="W3.ROSTER">
          <div className="basic-ui-section-title">
            <UiTag id="W3.ROSTER" />
            <strong>编制士兵花名册（{soldiersList.length} 名，点击查看个人情况与物品栏）</strong>
          </div>
          <div className="basic-ui-scroll-box">
            <table className="basic-ui-table basic-ui-table-hover">
              <thead>
                <tr>
                  <th>ID</th>
                  <th>姓名</th>
                  <th>职务</th>
                  <th>状态</th>
                  <th>生命</th>
                  <th>主武器</th>
                  <th>操作</th>
                </tr>
              </thead>
              <tbody>
                {soldiersList.map((s) => {
                  const isOpen = selectedSoldierId === s.soldier_id;
                  const primaryWeapon = s.inventory?.find((it) => it.category === 'weapon')?.name ?? '制式步枪';
                  const sTagId = `W3.S${s.soldier_id}`;
                  return (
                    <tr
                      key={s.soldier_id}
                      data-testid={`soldier-row-${s.soldier_id}`}
                      data-ui-id={sTagId}
                      className={isOpen ? 'basic-row-active' : undefined}
                      onClick={() => setSelectedSoldierId(isOpen ? null : s.soldier_id)}
                    >
                      <td>
                        <UiTag id={sTagId} />
                      </td>
                      <td>{s.name ?? `士兵 #${s.soldier_id}`}</td>
                      <td>{s.roleLabel ?? '步枪兵'}</td>
                      <td>{!s.alive ? '阵亡' : s.bleed_rate_per_tick > 0 ? '负伤' : '正常'}</td>
                      <td>
                        {Math.round(s.hp)}/{s.max_hp}
                      </td>
                      <td>{primaryWeapon}</td>
                      <td>
                        <button
                          type="button"
                          className="basic-ui-btn"
                          data-testid={`btn-soldier-${s.soldier_id}`}
                        >
                          {isOpen ? '收起' : '详情/物品'}
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>

        {/* 三、选中士兵详情与个人物品栏 */}
        {activeSoldier && (
          <div
            className="basic-ui-section basic-ui-subbox"
            data-testid={`soldier-detail-${activeSoldier.soldier_id}`}
            data-ui-id={`W3.SD.${activeSoldier.soldier_id}`}
          >
            <div className="basic-ui-section-title">
              <UiTag id={`W3.SD.${activeSoldier.soldier_id}`} />
              <strong>
                单兵情况：#{activeSoldier.soldier_id} {activeSoldier.name}（{activeSoldier.roleLabel}）
              </strong>
            </div>

            <div className="basic-ui-kv-row" data-ui-id={`W3.SD.${activeSoldier.soldier_id}.BODY`}>
              <UiTag id={`W3.SD.${activeSoldier.soldier_id}.BODY`} />
              <span>状态：{activeSoldier.alive ? '在位作战' : '阵亡'}</span>
              <span>压制：{activeSoldier.suppression.toFixed(1)}</span>
              <span>失血：{activeSoldier.bleed_rate_per_tick.toFixed(1)}/s</span>
            </div>

            <table className="basic-ui-table">
              <thead>
                <tr>
                  {BODY_REGION_NAMES.map((name, idx) => (
                    <th key={idx}>{name}</th>
                  ))}
                </tr>
              </thead>
              <tbody>
                <tr>
                  {activeSoldier.regions.map((r, idx) => (
                    <td key={idx}>
                      {Math.round(r.integrity * 100)}%
                      {r.cumulative_damage > 0 ? ` (-${r.cumulative_damage.toFixed(1)})` : ''}
                    </td>
                  ))}
                </tr>
              </tbody>
            </table>

            <div
              className="basic-ui-section-title"
              style={{ marginTop: '6px' }}
              data-ui-id={`W3.INV.${activeSoldier.soldier_id}`}
            >
              <UiTag id={`W3.INV.${activeSoldier.soldier_id}`} />
              <strong>个人物品栏（{activeSoldier.inventory?.length ?? 0} 项）</strong>
            </div>

            <table
              className="basic-ui-table"
              data-testid={`soldier-inventory-${activeSoldier.soldier_id}`}
            >
              <thead>
                <tr>
                  <th>ID</th>
                  <th>类别</th>
                  <th>物品名称</th>
                  <th>数量</th>
                  <th>状态</th>
                </tr>
              </thead>
              <tbody>
                {(activeSoldier.inventory ?? []).map((item, idx) => {
                  const invId = `W3.INV.${activeSoldier.soldier_id}.${idx + 1}`;
                  return (
                    <tr key={item.id} data-ui-id={invId}>
                      <td>
                        <UiTag id={invId} />
                      </td>
                      <td>{INV_CATEGORY_LABEL[item.category] ?? item.category}</td>
                      <td>{item.name}</td>
                      <td>{item.count}</td>
                      <td>{item.status}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </section>
  );
};

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

  const strengthColor = selected?.team === 'red' ? '#436779' : '#a94f43';
  const orgPct = selected ? Math.max(0, 100 - (selected.suppression ?? 0)) : 100;
  const ammoPct = selected && (selected.maxAmmo ?? 0) > 0 ? Math.min(100, Math.round(((selected.ammo ?? 0) / selected.maxAmmo) * 100)) : 100;

  return (
    <div className="inspector-body-root">
      {/* 头部军规呼号 (UI-03) */}
      <div className="inspector-heading-row">
        <span className="inspector-eyebrow">UNIT / INSPECTOR</span>
        <span className="inspector-callsign">F-031{selected?.id ?? 0}</span>
      </div>

      <div className="row" data-ui-id="I1" style={{ gap: '3px', padding: '0 2px' }}>
        <UiTag id="I1" />
        {units.map((u) => (
          <button
            key={u.id}
            type="button"
            data-testid={`select-unit-${u.id}`}
            className={u.id === selected?.id ? 'btn active' : 'btn'}
            style={{ padding: '2px 6px', fontSize: '11px' }}
            onClick={() => onSelectUnit(u.id)}
          >
            {u.name}
          </button>
        ))}
      </div>

      {selected && (
        <div className="inspector-stack">
          {/* 4174 单位铭牌 + APP-6 符号头 + 状态角标 */}
          <div className="inspector-card inspector-top-card">
            <div className="inspector-unit-display">
              <div
                className="inspector-app6-box"
                style={{
                  background: selected.team === 'red' ? '#4c6875' : '#6c423b',
                  borderColor: selected.team === 'red' ? '#c2d4d9' : '#ddab8f',
                }}
              >
                <span className="inspector-app6-echelon">
                  {selected.role === 'platoon_hq' ? 'III' : selected.isVehicle ? 'I' : 'II'}
                </span>
                <svg width="32" height="22" viewBox="0 0 32 22">
                  {selected.isVehicle ? (
                    <rect x="6" y="5" width="20" height="12" rx="6" fill="none" stroke="#c2d4d9" strokeWidth="1.4" />
                  ) : selected.role === 'platoon_hq' ? (
                    <text x="16" y="15" textAnchor="middle" fill="#c2d4d9" fontSize="10" fontFamily="Consolas, monospace">
                      HQ
                    </text>
                  ) : (
                    <>
                      <line x1="2" y1="2" x2="30" y2="20" stroke="#c2d4d9" strokeWidth="1.4" />
                      <line x1="30" y1="2" x2="2" y2="20" stroke="#c2d4d9" strokeWidth="1.4" />
                    </>
                  )}
                </svg>
              </div>
              <div className="inspector-unit-meta">
                <div className="inspector-unit-name" data-ui-id="I2">
                  <UiTag id="I2" />
                  <span>{selected.name}</span>
                </div>
                <span className="inspector-unit-code">
                  {selected.id} / {ROLE_LABELS[selected.role] ?? selected.role}
                </span>
              </div>
            </div>
            <div className="inspector-status-tag" style={{ borderColor: selected.suppression > 40 ? '#a94f43' : '#56695e' }}>
              <i style={{ background: selected.suppression > 40 ? '#c94a3d' : '#a2b89b' }} />
              {selected.activity.type === 'idle' || selected.activity.type === 'hold' ? '待命 · 通信正常' : `执行任务：${selected.activity.type}`}
            </div>
          </div>

          {/* 4174 UI-03B 作战实力 COMBAT STRENGTH (大字等宽 + 分段刻度进度条) */}
          <div className="inspector-card" data-ui-id="I3">
            <div className="inspector-section-title">
              <span>
                <UiTag id="I3" />
                作战实力
              </span>
              <small>COMBAT STRENGTH</small>
            </div>
            <div className="inspector-strength">
              <b data-testid="inspector-soldiers">{selected.soldiers}</b>
              <span>/ {selected.maxSoldiers}</span>
              <label>在位人员</label>
            </div>

            <div className="inspector-metric">
              <div className="inspector-metric-head">
                <span>组织度 / 压制</span>
                <b data-testid="inspector-suppression">
                  {orgPct}% (压制 {selected.suppression}%)
                </b>
              </div>
              <div className="inspector-metric-track">
                <div
                  className="inspector-metric-fill"
                  style={{
                    width: `${orgPct}%`,
                    background:
                      selected.suppression > 50
                        ? 'repeating-linear-gradient(90deg,#a94f43 0,#a94f43 16px,transparent 16px,transparent 19px)'
                        : undefined,
                  }}
                />
              </div>
            </div>

            <div className="inspector-metric amber">
              <div className="inspector-metric-head">
                <span>弹药储备</span>
                <b data-testid="inspector-ammo">
                  {selected.ammo}/{selected.maxAmmo} ({ammoPct}%)
                </b>
              </div>
              <div className="inspector-metric-track">
                <div className="inspector-metric-fill" style={{ width: `${ammoPct}%` }} />
              </div>
            </div>

            <div className="inspector-metric">
              <div className="inspector-metric-head">
                <span>工事 / 作业进度</span>
                <b>{Math.round(selected.workProgress * 100)}%</b>
              </div>
              <div className="inspector-metric-track">
                <div className="inspector-metric-fill" style={{ width: `${Math.round(selected.workProgress * 100)}%` }} />
              </div>
            </div>
          </div>

          {/* 4174 UI-03C 位置与环境 POSITION */}
          <div className="inspector-card">
            <div className="inspector-section-title">
              <span>位置与环境</span>
              <small>POSITION</small>
            </div>
            <div className="inspector-data-grid">
              <div className="inspector-datum">
                <label>战区坐标</label>
                <b data-testid="inspector-pos">
                  ({selected.pos.x.toFixed(1)}, {selected.pos.y.toFixed(1)}) m
                </b>
              </div>
              <div className="inspector-datum">
                <label>交战纪律</label>
                <b>{selected.discipline}</b>
              </div>
              <div className="inspector-datum">
                <label>指挥通联</label>
                <b style={{ color: delay?.remote ? '#d8b46e' : '#b7c8ad' }}>
                  {delay?.remote ? `无线电 +${delay.delayTicks}拍` : '直连 0s'}
                </b>
              </div>
              <div className="inspector-datum">
                <label>阵营识别</label>
                <b style={{ color: strengthColor === '#436779' ? '#a6c4cf' : '#df9b8e' }}>
                  {selected.team === 'red' ? '己方实报' : '敌情推定'}
                </b>
              </div>
            </div>
          </div>

          {/* 4174 UI-03D 当前意图 COMMAND INTENT (竖线 + 金点卡) */}
          <div className="inspector-card">
            <div className="inspector-section-title">
              <span>当前意图</span>
              <small>COMMAND INTENT</small>
            </div>
            <div className="inspector-order-card">
              <div className="inspector-order-kind">当前状态：{selected.activity.type}</div>
              <p>点击地图上对应兵牌可调出单兵部位与个人物品清单（W3）；按 M 展开参谋印刷副图。</p>
            </div>
          </div>

          <div className="meta-note" data-ui-id="I4" style={{ fontSize: '10px' }}>
            <UiTag id="I4" />
            动作：{selected.activity.type} · 作业 {Math.round(selected.workProgress * 100)}% · {selected.discipline}
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
  <div data-ui-id="G1" style={{ display: 'flex', flexDirection: 'column', gap: '3px', fontSize: '11px', fontFamily: 'monospace' }}>
    <UiTag id="G1" />
    {log.length === 0 ? (
      <span className="meta-note" style={{ color: '#556877', fontStyle: 'italic' }}>
        等待走表推进 (WAITING FOR TIME STEP)...
      </span>
    ) : (
      log.map((item, idx) => (
        <div key={`${item.ms}-${idx}`} style={{ display: 'flex', gap: '6px', color: '#b0c4d2', borderLeft: '2px solid rgba(82, 108, 124, 0.4)', paddingLeft: '5px' }}>
          <span style={{ color: '#c89f55', fontWeight: 600 }}>+{(item.ms / 1000).toFixed(1)}s</span>
          <span>{item.text}</span>
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
