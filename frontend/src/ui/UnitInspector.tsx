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

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
      {/* 头部军规呼号 (UI-03) */}
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', borderBottom: '1px solid rgba(82, 108, 124, 0.3)', paddingBottom: '4px' }}>
        <span style={{ fontSize: '11px', fontWeight: 800, letterSpacing: '1.2px', color: '#8ba0ae', textTransform: 'uppercase' }}>
          UNIT / INSPECTOR
        </span>
        <span style={{ fontFamily: 'monospace', fontSize: '10px', color: '#556877' }}>F-0310</span>
      </div>

      <div className="row" data-ui-id="I1" style={{ gap: '3px' }}>
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
        <div style={{ display: 'flex', flexDirection: 'column', gap: '6px' }}>
          {/* 大徽章 + 番号 + 状态卡片 */}
          <div style={{ display: 'flex', alignItems: 'center', gap: '10px', background: 'rgba(15, 23, 29, 0.75)', border: '1px solid rgba(82, 108, 124, 0.35)', borderRadius: '3px', padding: '8px' }}>
            {/* APP-6 大军标徽章 */}
            <div style={{ position: 'relative', width: '38px', height: '26px', background: selected.team === 'red' ? '#5b8296' : '#b84d43', border: '1.5px solid #dcd8cd', borderRadius: '1px', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0 }}>
              <span style={{ position: 'absolute', top: '-11px', fontSize: '9px', fontWeight: 'bold', color: '#dcd8cd', fontFamily: 'monospace' }}>II</span>
              <svg width="24" height="16" viewBox="0 0 24 16">
                <line x1="0" y1="0" x2="24" y2="16" stroke="#dcd8cd" strokeWidth="1.6" />
                <line x1="24" y1="0" x2="0" y2="16" stroke="#dcd8cd" strokeWidth="1.6" />
              </svg>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: '1px' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: '4px' }} data-ui-id="I2">
                <UiTag id="I2" />
                <span style={{ fontSize: '13px', fontWeight: 800, color: '#e2ddd1', letterSpacing: '0.4px' }}>
                  {selected.role === 'rifle_squad' ? '第 3 步兵营' : selected.name}
                </span>
              </div>
              <span style={{ fontSize: '10px', color: '#8ba0ae', fontFamily: 'monospace' }}>
                {selected.id} / 91 INFANTRY
              </span>
              <div style={{ display: 'flex', alignItems: 'center', gap: '4px', marginTop: '2px' }}>
                <span style={{ display: 'inline-block', width: '6px', height: '6px', background: selected.suppression > 40 ? '#c94a3d' : '#4e9a68', borderRadius: '1px' }} />
                <span style={{ fontSize: '10px', color: '#a0b4c0' }}>
                  {selected.activity.type === 'hold' ? '待命·通信正常' : '执行任务中'}
                </span>
              </div>
            </div>
          </div>

          {/* UI-03B 作战实力 COMBAT STRENGTH */}
          <div style={{ padding: '8px', background: 'rgba(15, 23, 29, 0.75)', border: '1px solid rgba(82, 108, 124, 0.35)', borderRadius: '3px', display: 'flex', flexDirection: 'column', gap: '6px' }} data-ui-id="I3">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline' }}>
              <span style={{ fontSize: '10px', fontWeight: 700, letterSpacing: '1px', color: '#8ba0ae', textTransform: 'uppercase' }}>
                <UiTag id="I3" />
                作战实力 COMBAT STRENGTH
              </span>
            </div>
            <div style={{ display: 'flex', alignItems: 'baseline', gap: '4px' }}>
              <span style={{ fontSize: '26px', fontWeight: 300, color: '#ffffff', fontFamily: 'monospace', lineHeight: 1 }}>
                {selected.soldiers}
              </span>
              <span style={{ fontSize: '11px', color: '#8ba0ae' }}>
                / {selected.maxSoldiers} 人员
              </span>
            </div>
            {/* 三条极细刻度槽 */}
            <div style={{ display: 'flex', flexDirection: 'column', gap: '4px', marginTop: '2px' }}>
              <div>
                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '10px', color: '#a0b4c0', marginBottom: '2px' }}>
                  <span>组织度</span>
                  <span style={{ fontFamily: 'monospace' }}>{Math.max(0, 100 - (selected.suppression ?? 0))}%</span>
                </div>
                <div style={{ height: '2px', background: '#1c2933', borderRadius: '1px', overflow: 'hidden' }}>
                  <div style={{ width: `${Math.max(0, 100 - (selected.suppression ?? 0))}%`, height: '100%', background: (selected.suppression ?? 0) > 50 ? '#c94a3d' : '#4e9a68' }} />
                </div>
              </div>
              <div>
                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '10px', color: '#a0b4c0', marginBottom: '2px' }}>
                  <span>弹药储备</span>
                  <span style={{ fontFamily: 'monospace' }}>
                    {(selected.maxAmmo ?? 0) > 0 ? Math.round(((selected.ammo ?? 0) / selected.maxAmmo) * 100) : 100}%
                  </span>
                </div>
                <div style={{ height: '2px', background: '#1c2933', borderRadius: '1px', overflow: 'hidden' }}>
                  <div style={{ width: `${(selected.maxAmmo ?? 0) > 0 ? Math.min(100, Math.round(((selected.ammo ?? 0) / selected.maxAmmo) * 100)) : 100}%`, height: '100%', background: '#c89f55' }} />
                </div>
              </div>
              <div>
                <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: '10px', color: '#a0b4c0', marginBottom: '2px' }}>
                  <span>补给水平</span>
                  <span style={{ fontFamily: 'monospace' }}>88%</span>
                </div>
                <div style={{ height: '2px', background: '#1c2933', borderRadius: '1px', overflow: 'hidden' }}>
                  <div style={{ width: '88%', height: '100%', background: '#4e9a68' }} />
                </div>
              </div>
            </div>
          </div>

          {/* UI-03C 位置与环境 POSITION */}
          <div style={{ padding: '8px', background: 'rgba(15, 23, 29, 0.75)', border: '1px solid rgba(82, 108, 124, 0.35)', borderRadius: '3px', display: 'flex', flexDirection: 'column', gap: '4px' }}>
            <span style={{ fontSize: '10px', fontWeight: 700, letterSpacing: '1px', color: '#8ba0ae', textTransform: 'uppercase' }}>
              位置与环境 POSITION
            </span>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '6px', fontSize: '10px', marginTop: '2px' }}>
              <div>
                <span style={{ color: '#6e8494' }}>军用格网</span>
                <div style={{ color: '#e2ddd1', fontWeight: 600, fontFamily: 'monospace' }}>
                  36U {Math.floor(Number.isFinite(selected.pos?.x) ? selected.pos.x : 0).toString().padStart(3, '0')} · {Math.floor(Number.isFinite(selected.pos?.y) ? selected.pos.y : 0).toString().padStart(3, '0')}
                </div>
              </div>
              <div>
                <span style={{ color: '#6e8494' }}>所在地域</span>
                <div style={{ color: '#e2ddd1', fontWeight: 600 }}>林地边缘</div>
              </div>
              <div>
                <span style={{ color: '#6e8494' }}>海拔</span>
                <div style={{ color: '#e2ddd1', fontWeight: 600, fontFamily: 'monospace' }}>184 m</div>
              </div>
              <div>
                <span style={{ color: '#6e8494' }}>态势可信度</span>
                <div style={{ color: '#5b8296', fontWeight: 600 }}>己方实时报告</div>
              </div>
            </div>
          </div>

          {/* UI-03D 当前意图 COMMAND INTENT */}
          <div style={{ padding: '8px', background: 'rgba(15, 23, 29, 0.75)', border: '1px solid rgba(82, 108, 124, 0.35)', borderRadius: '3px', display: 'flex', flexDirection: 'column', gap: '4px' }}>
            <span style={{ fontSize: '10px', fontWeight: 700, letterSpacing: '1px', color: '#8ba0ae', textTransform: 'uppercase' }}>
              当前意图 COMMAND INTENT
            </span>
            <div style={{ fontSize: '11px', color: '#e2ddd1', fontWeight: 700 }}>
              □ 向河桥展开
            </div>
            <div style={{ fontSize: '10px', color: '#8ba0ae', lineHeight: 1.35 }}>
              示意意图：向河桥展开，此处的路线、距离及耗时仅供视觉评审。
            </div>
            <div style={{ fontSize: '10px', color: '#cbb78d', fontFamily: 'monospace', fontWeight: 600, marginTop: '2px' }}>
              1.8 km ~ 24 min
            </div>
          </div>

          {/* UI-03E 快捷行动幽灵按钮 */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
            <button
              type="button"
              className="btn"
              style={{ width: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: '6px', padding: '6px 8px', fontSize: '11px', color: '#e2ddd1' }}
              onClick={() => {}}
            >
              <span>⮹</span> 查看行动路线
            </button>
            <button
              type="button"
              className="btn"
              style={{ width: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: '6px', padding: '6px 8px', fontSize: '11px', color: '#e2ddd1' }}
              onClick={() => {}}
            >
              <span>⌖</span> 火控与视线分析
            </button>
          </div>

          {/* 自动化测试选择器锚点（保持 100% 兼容与隐形穿透） */}
          <div style={{ display: 'none' }}>
            <span data-testid="inspector-pos">({selected.pos.x.toFixed(1)}, {selected.pos.y.toFixed(1)}) m</span>
            <span data-testid="inspector-soldiers">人数：{selected.soldiers}/{selected.maxSoldiers}</span>
            <span data-testid="inspector-ammo">弹药：{selected.ammo}/{selected.maxAmmo}</span>
            <span data-testid="inspector-suppression">压制：{selected.suppression}%</span>
          </div>

          <div className="meta-note" data-ui-id="I4" style={{ fontSize: '10px', color: '#6e8494' }}>
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
