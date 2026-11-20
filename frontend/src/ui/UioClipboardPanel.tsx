// GenTactix V2 frontend · 手写板（UIO）四区面板：回形针波次栏 / 任务队列栏 / 草稿栏 / 归档栏

import React, { useState } from 'react';
import {
  MACRO_LABEL,
  TASK_KIND_LABEL,
  checkQueueGate,
  computeCommandDelay,
  effectiveWindow,
  remoteDelayText,
  requiredTargetType,
} from '../engine/directives';
import type {
  DirectiveMode,
  DirectiveTarget,
  DirectiveTaskKind,
  EngineerWorkKind,
  FireDiscipline,
  HeadlessDirective,
  MoveStance,
  UnitState,
  WaveDirective,
} from '../engine/types';
import { queueConflicts, type UioState } from '../store/uioStore';

export interface UioClipboardPanelProps {
  uio: UioState;
  units: UnitState[];
  hqUnitId: number;
  directRadiusM: number;
  waves: WaveDirective[];
  activeWaveId?: string;
  selectedUnitId?: number;
  mapPickTarget?: DirectiveTarget;
  isPickingTarget: boolean;
  onToggleWave: (waveId: string) => void;
  onTogglePickTarget: (active: boolean) => void;
  onClearPickedTarget: () => void;
  onCreateDraft: (opts: {
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
  }) => void;
  onBindDraftUnit: (draftId: string, unitId: number | undefined) => void;
  onBindDraftTarget: (draftId: string, target: DirectiveTarget | undefined) => void;
  onMoveToQueue: (draftId: string) => void;
  onMoveToDraft: (queueId: string) => void;
  onDeleteDraft: (draftId: string) => void;
  onCopyToDraft: (id: string) => void;
  onRequeueArchive: (archiveId: string) => void;
  onIssueQueue: () => void;
}

function formatTarget(t: DirectiveTarget | undefined): string {
  if (!t) return '未绑定标的物';
  if (t.type === 'point') return `坐标 (${Math.round(t.pos.x)}, ${Math.round(t.pos.y)})`;
  if (t.type === 'polyline') return `折线 ${t.points.length} 点`;
  return `目标单位 #${t.unitId}`;
}

function directiveTitle(d: HeadlessDirective): string {
  return d.mode === 'macro' ? MACRO_LABEL : TASK_KIND_LABEL[d.taskKind ?? 'move_unit'];
}

export const UioClipboardPanel: React.FC<UioClipboardPanelProps> = ({
  uio,
  units,
  hqUnitId,
  directRadiusM,
  waves,
  activeWaveId,
  selectedUnitId,
  mapPickTarget,
  isPickingTarget,
  onToggleWave,
  onTogglePickTarget,
  onClearPickedTarget,
  onCreateDraft,
  onBindDraftUnit,
  onBindDraftTarget,
  onMoveToQueue,
  onMoveToDraft,
  onDeleteDraft,
  onCopyToDraft,
  onRequeueArchive,
  onIssueQueue,
}) => {
  const [mode, setMode] = useState<DirectiveMode>('task');
  const [taskKind, setTaskKind] = useState<DirectiveTaskKind>('move_unit');
  const [bindSelectedUnit, setBindSelectedUnit] = useState<boolean>(false);
  const [startSec, setStartSec] = useState<number>(0);
  const [endSec, setEndSec] = useState<number>(12);
  const [delayOffsetSec, setDelayOffsetSec] = useState<number>(0);
  const [delayMultiplier, setDelayMultiplier] = useState<number>(1);
  const [stance, setStance] = useState<MoveStance>('normal');
  const [discipline, setDiscipline] = useState<FireDiscipline>('return_fire');
  const [work, setWork] = useState<EngineerWorkKind>('foxhole_line');

  const hq = units.find((u) => u.id === hqUnitId);
  const friendlyUnits = units.filter((u) => u.team === 'red');
  const conflicts = queueConflicts(uio);
  const canIssue = uio.queue.length > 0 && conflicts.size === 0;

  const handleCreate = () => {
    onCreateDraft({
      mode,
      taskKind,
      unitId: bindSelectedUnit ? selectedUnitId : undefined,
      target: mapPickTarget,
      startSec,
      endSec,
      delayOffsetSec,
      delayMultiplier,
      stance,
      discipline,
      work,
    });
  };

  const renderDelayBadge = (unitId: number | undefined) => {
    if (unitId === undefined) return null;
    const u = units.find((x) => x.id === unitId);
    if (!u) return null;
    const info = computeCommandDelay(hq, u, directRadiusM);
    if (!info.remote) return <span className="meta-note">直接指挥（零电台延迟）</span>;
    return (
      <div className="remote-banner" data-testid={`remote-delay-${unitId}`}>
        {remoteDelayText(info)}
      </div>
    );
  };

  return (
    <div className="panel" data-testid="uio-panel" style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
      <div className="panel-header">
        <span>手写板 UIO（四区状态机）</span>
        <span className="meta-note">顶栏纸条仅作友军参考 · 排队列无需对齐</span>
      </div>

      {/* (1) 顶栏：回形针指挥纸条栏（只读波次栏） */}
      <div className="lane" data-testid="uio-wave-bar">
        <div className="lane-title">
          <span>📎 顶栏回形针指挥纸条（只读波次 · 点击投射至 Layer 5）</span>
          <span>{waves.length} 张</span>
        </div>
        <div className="paper-clip-row">
          {waves.map((w) => {
            const isActive = w.id === activeWaveId;
            return (
              <button
                key={w.id}
                type="button"
                data-testid={`wave-slip-${w.id}`}
                className={isActive ? 'wave-slip active' : 'wave-slip'}
                onClick={() => onToggleWave(w.id)}
              >
                <div>
                  <strong>{w.title}</strong>
                </div>
                <div>
                  G 场生效：#{w.effectiveTick} ({(w.effectiveExactMs / 1000).toFixed(1)}s){' '}
                  {w.effective ? '· 已生效' : w.remote ? `· 远程+${w.delayTicks}拍` : '· 直通'}
                </div>
              </button>
            );
          })}
        </div>
      </div>

      {/* 无头指令编辑器 */}
      <div className="lane" data-testid="uio-creator">
        <div className="lane-title">
          <span>新建无头指令 / 事件宏</span>
          <button
            type="button"
            data-testid="btn-create-draft"
            className="btn primary"
            onClick={handleCreate}
          >
            + 生成纸条至草稿栏
          </button>
        </div>

        <div className="row">
          <select
            aria-label="指令形态"
            data-testid="select-mode"
            className="select"
            value={mode}
            onChange={(e) => setMode(e.target.value as DirectiveMode)}
          >
            <option value="task">单任务</option>
            <option value="macro">事件宏（前出→散兵坑→警戒）</option>
          </select>

          {mode === 'task' && (
            <select
              aria-label="任务类型"
              data-testid="select-task-kind"
              className="select"
              value={taskKind}
              onChange={(e) => setTaskKind(e.target.value as DirectiveTaskKind)}
            >
              <option value="move_unit">机动 Move</option>
              <option value="attack_pos">火力压制 AttackPos</option>
              <option value="attack_unit">攻击单位 AttackUnit</option>
              <option value="build_work">构筑工事 BuildWork</option>
              <option value="pop_smoke">施放烟幕 PopSmoke</option>
              <option value="set_fire_discipline">开火纪律</option>
              <option value="hold_unit">停止待命 Hold</option>
            </select>
          )}

          {mode === 'task' && taskKind === 'build_work' && (
            <select
              aria-label="工事类型"
              className="select"
              value={work}
              onChange={(e) => setWork(e.target.value as EngineerWorkKind)}
            >
              <option value="foxhole_line">散兵坑线</option>
              <option value="zigzag_trench">曲折堑壕</option>
              <option value="minefield">雷场</option>
              <option value="demolition">爆破准备</option>
            </select>
          )}

          <select
            aria-label="行军姿态"
            className="select"
            value={stance}
            onChange={(e) => setStance(e.target.value as MoveStance)}
          >
            <option value="normal">正常行军</option>
            <option value="fast">急行军</option>
            <option value="stealth">隐蔽匍匐</option>
          </select>

          <select
            aria-label="开火纪律"
            className="select"
            value={discipline}
            onChange={(e) => setDiscipline(e.target.value as FireDiscipline)}
          >
            <option value="return_fire">还击</option>
            <option value="fire_at_will">自由开火</option>
            <option value="hold_fire">严守停火</option>
          </select>
        </div>

        <div className="row">
          <label className="meta-note">
            起始(s)
            <input
              type="number"
              data-testid="input-start-sec"
              className="input"
              style={{ width: '54px', marginLeft: '4px' }}
              value={startSec}
              min={0}
              onChange={(e) => setStartSec(Number(e.target.value))}
            />
          </label>
          <label className="meta-note">
            结束(s)
            <input
              type="number"
              data-testid="input-end-sec"
              className="input"
              style={{ width: '54px', marginLeft: '4px' }}
              value={endSec}
              min={1}
              onChange={(e) => setEndSec(Number(e.target.value))}
            />
          </label>
          <label className="meta-note">
            延迟偏移(s)
            <input
              type="number"
              data-testid="input-delay-offset"
              className="input"
              style={{ width: '54px', marginLeft: '4px' }}
              value={delayOffsetSec}
              onChange={(e) => setDelayOffsetSec(Number(e.target.value))}
            />
          </label>
          <label className="meta-note">
            乘数
            <input
              type="number"
              step="0.5"
              min="0.5"
              className="input"
              style={{ width: '50px', marginLeft: '4px' }}
              value={delayMultiplier}
              onChange={(e) => setDelayMultiplier(Number(e.target.value))}
            />
          </label>
        </div>

        <div className="row">
          <label className="meta-note">
            <input
              type="checkbox"
              checked={bindSelectedUnit}
              onChange={(e) => setBindSelectedUnit(e.target.checked)}
            />{' '}
            自动绑定当前选中单位（{selectedUnitId ? `#${selectedUnitId}` : '无'}）
          </label>
          <button
            type="button"
            data-testid="btn-pick-target"
            className={isPickingTarget ? 'btn active' : 'btn'}
            onClick={() => onTogglePickTarget(!isPickingTarget)}
          >
            {isPickingTarget ? '点击地图选标中...' : '地图拾取标的物'}
          </button>
          {mapPickTarget && (
            <button type="button" className="btn" onClick={onClearPickedTarget}>
              清除标({formatTarget(mapPickTarget)})
            </button>
          )}
        </div>
      </div>

      {uio.gateError && (
        <div className="warning-banner" data-testid="uio-gate-error">
          {uio.gateError}
        </div>
      )}

      {/* (2) 任务队列栏（双重签发门禁 + 互斥检测） */}
      <div
        className="lane"
        data-testid="uio-queue-lane"
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => {
          e.preventDefault();
          const archivedId = e.dataTransfer.getData('text/gentactix-archive-id');
          if (archivedId) onRequeueArchive(archivedId);
        }}
      >
        <div className="lane-title">
          <span>📋 任务队列栏（{uio.queue.length} 待签发 / {uio.locked.length} 已锁死）</span>
          <button
            type="button"
            data-testid="btn-issue-queue"
            className="btn primary"
            disabled={!canIssue}
            onClick={onIssueQueue}
          >
            签发任务队列
          </button>
        </div>

        {uio.queue.map((d) => {
          const u = units.find((x) => x.id === d.unitId);
          const win = effectiveWindow(d);
          const cs = conflicts.get(d.id);
          return (
            <div key={d.id} data-testid={`queue-card-${d.id}`} className={cs ? 'card conflict' : 'card'}>
              <div className="row" style={{ justifyContent: 'space-between' }}>
                <strong>
                  #{d.seq} {directiveTitle(d)}
                </strong>
                <span className="meta-note">
                  {u?.name ?? `#${d.unitId}`} · [{win.start}s ~ {win.end}s]
                </span>
              </div>
              <div className="meta-note">{formatTarget(d.target)}</div>
              {renderDelayBadge(d.unitId)}
              {cs && (
                <div className="warning-banner" data-testid={`conflict-alert-${d.id}`}>
                  时间槽冲突：同一单位与 {cs.map((c) => c.otherId).join(', ')} 在 [{cs[0]!.start}s ~ {cs[0]!.end}s] 重叠
                </div>
              )}
              <div className="row">
                <button type="button" className="btn" onClick={() => onMoveToDraft(d.id)}>
                  退回草稿
                </button>
                <button type="button" className="btn" onClick={() => onCopyToDraft(d.id)}>
                  复制副本到草稿栏
                </button>
              </div>
            </div>
          );
        })}

        {uio.locked.map(({ directive: d, receipt }) => {
          const u = units.find((x) => x.id === d.unitId);
          return (
            <div key={d.id} data-testid={`locked-card-${d.id}`} className="card locked">
              <div className="row" style={{ justifyContent: 'space-between' }}>
                <strong>
                  🔒 #{d.seq} {directiveTitle(d)}（已签发锁死）
                </strong>
                <span className="meta-note">{u?.name}</span>
              </div>
              <div className="meta-note">
                槽位 #{receipt.lutSlot} ({receipt.category}) · 生效 #{receipt.firstTick} ({(receipt.firstExactMs / 1000).toFixed(1)}s) → 结束 #{receipt.endTick}{' '}
                {receipt.remote ? `· 含远程+${receipt.delayTicks}拍` : ''}
              </div>
              <div className="row">
                <button type="button" className="btn" onClick={() => onCopyToDraft(d.id)}>
                  复制副本到草稿栏
                </button>
              </div>
            </div>
          );
        })}
      </div>

      {/* (3) 草稿栏（无约束 + 模板复用） */}
      <div className="lane" data-testid="uio-draft-lane">
        <div className="lane-title">
          <span>📝 草稿栏（{uio.drafts.length}）</span>
          <span className="meta-note">可放未绑定草稿与历史副本</span>
        </div>
        {uio.drafts.map((d) => {
          const gate = checkQueueGate(d);
          const req = requiredTargetType(d);
          const win = effectiveWindow(d);
          return (
            <div key={d.id} data-testid={`draft-card-${d.id}`} className="card">
              <div className="row" style={{ justifyContent: 'space-between' }}>
                <strong>
                  #{d.seq} {directiveTitle(d)} {d.origin === 'copy' ? '(副本)' : ''}
                </strong>
                <span className="meta-note">
                  [{win.start}s ~ {win.end}s]
                </span>
              </div>
              <div className="row">
                <select
                  aria-label={`绑定单位-${d.id}`}
                  data-testid={`bind-unit-${d.id}`}
                  className="select"
                  value={d.unitId ?? ''}
                  onChange={(e) => onBindDraftUnit(d.id, e.target.value ? Number(e.target.value) : undefined)}
                >
                  <option value="">-- 未绑定执行单位 --</option>
                  {friendlyUnits.map((u) => (
                    <option key={u.id} value={u.id}>
                      {u.name}
                    </option>
                  ))}
                </select>
                {req !== null && (
                  <button
                    type="button"
                    data-testid={`bind-target-${d.id}`}
                    className="btn"
                    disabled={!mapPickTarget}
                    onClick={() => mapPickTarget && onBindDraftTarget(d.id, mapPickTarget)}
                  >
                    绑定地图拾取标
                  </button>
                )}
                {req === 'unit' && (
                  <button
                    type="button"
                    className="btn"
                    onClick={() => {
                      const enemy = units.find((u) => u.team !== 'red');
                      if (enemy) onBindDraftTarget(d.id, { type: 'unit', unitId: enemy.id });
                    }}
                  >
                    标定敌方
                  </button>
                )}
              </div>
              <div className="meta-note">
                标的物：{formatTarget(d.target)} · {gate.ok ? '门禁就绪' : gate.message}
              </div>
              {renderDelayBadge(d.unitId)}
              <div className="row">
                <button
                  type="button"
                  data-testid={`send-to-queue-${d.id}`}
                  className="btn primary"
                  onClick={() => onMoveToQueue(d.id)}
                >
                  送入任务队列
                </button>
                <button type="button" className="btn danger" onClick={() => onDeleteDraft(d.id)}>
                  删除
                </button>
              </div>
            </div>
          );
        })}
      </div>

      {/* (4) 归档栏（执行完毕自动流转 + 支持鼠标拖拽回队列） */}
      <div className="lane" data-testid="uio-archive-lane">
        <div className="lane-title">
          <span>📦 归档栏（{uio.archive.length}）</span>
          <span className="meta-note">可拖拽回任务队列栏重用</span>
        </div>
        {uio.archive.map((d) => {
          const u = units.find((x) => x.id === d.unitId);
          return (
            <div
              key={d.id}
              data-testid={`archive-card-${d.id}`}
              className="card"
              draggable
              onDragStart={(e) => e.dataTransfer.setData('text/gentactix-archive-id', d.id)}
            >
              <div className="row" style={{ justifyContent: 'space-between' }}>
                <strong>
                  ✓ #{d.seq} {directiveTitle(d)}
                </strong>
                <span className="meta-note">{u?.name}</span>
              </div>
              <div className="meta-note">{formatTarget(d.target)}</div>
              <div className="row">
                <button
                  type="button"
                  data-testid={`requeue-${d.id}`}
                  className="btn primary"
                  onClick={() => onRequeueArchive(d.id)}
                >
                  拖回任务队列
                </button>
                <button type="button" className="btn" onClick={() => onCopyToDraft(d.id)}>
                  复制副本到草稿栏
                </button>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
};
