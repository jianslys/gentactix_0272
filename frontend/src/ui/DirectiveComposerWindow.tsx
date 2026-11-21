// GenTactix V2 frontend · 独立【无头指令编写器】（W12）
// 职责：只管写纸条（选单任务/事件宏、设起止时间与延迟、可选绑单位/标的物），写好后撕下送到手写板 W5 草稿栏。

import React, { useEffect, useState } from 'react';
import type {
  DirectiveMode,
  DirectiveTarget,
  DirectiveTaskKind,
  EngineerWorkKind,
  FireDiscipline,
  HeadlessDirective,
  MoveStance,
  UnitState,
} from '../engine/types';
import { UiIdContext, UiTag } from './UiTag';

export interface DirectiveComposerWindowProps {
  units: UnitState[];
  selectedUnitId?: number;
  mapPickTarget?: DirectiveTarget;
  isPickingTarget: boolean;
  editingDraft?: HeadlessDirective;
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
  onSaveEditedDraft?: (draftId: string, patch: Partial<HeadlessDirective>) => void;
  onCancelEdit?: () => void;
}

function formatTarget(t: DirectiveTarget | undefined): string {
  if (!t) return '未指明标的物';
  if (t.type === 'point') return `(${Math.round(t.pos.x)}, ${Math.round(t.pos.y)})`;
  if (t.type === 'polyline') return `折线 ${t.points.length} 点`;
  return `敌#${t.unitId}`;
}

export const DirectiveComposerWindow: React.FC<DirectiveComposerWindowProps> = ({
  units,
  selectedUnitId,
  mapPickTarget,
  isPickingTarget,
  editingDraft,
  onTogglePickTarget,
  onClearPickedTarget,
  onCreateDraft,
  onSaveEditedDraft,
  onCancelEdit,
}) => {
  const showIds = React.useContext(UiIdContext);
  const optTxt = (optId: string, label: string) => (showIds ? `[${optId}] ${label}` : label);

  const [mode, setMode] = useState<DirectiveMode>('task');
  const [taskKind, setTaskKind] = useState<DirectiveTaskKind>('move_unit');
  const [bindSelectedUnit, setBindSelectedUnit] = useState<boolean>(false);
  const [explicitUnitId, setExplicitUnitId] = useState<number | undefined>(undefined);
  const [startSec, setStartSec] = useState<number>(0);
  const [endSec, setEndSec] = useState<number>(12);
  const [delayOffsetSec, setDelayOffsetSec] = useState<number>(0);
  const [delayMultiplier, setDelayMultiplier] = useState<number>(1);
  const [stance, setStance] = useState<MoveStance>('normal');
  const [discipline, setDiscipline] = useState<FireDiscipline>('return_fire');
  const [work, setWork] = useState<EngineerWorkKind>('foxhole_line');

  useEffect(() => {
    if (!editingDraft) return;
    setMode(editingDraft.mode);
    setTaskKind(editingDraft.taskKind ?? 'move_unit');
    setExplicitUnitId(editingDraft.unitId);
    setBindSelectedUnit(false);
    setStartSec(editingDraft.startSec);
    setEndSec(editingDraft.endSec);
    setDelayOffsetSec(editingDraft.delayOffsetSec);
    setDelayMultiplier(editingDraft.delayMultiplier);
    setStance(editingDraft.stance);
    setDiscipline(editingDraft.discipline);
    setWork(editingDraft.work);
  }, [editingDraft]);

  const friendlyUnits = units.filter((u) => u.team === 'red');
  const resolvedUnitId = bindSelectedUnit ? selectedUnitId : explicitUnitId;

  const handleSubmit = () => {
    if (editingDraft && onSaveEditedDraft) {
      onSaveEditedDraft(editingDraft.id, {
        mode,
        taskKind,
        macroKind: mode === 'macro' ? 'advance_dig_watch' : undefined,
        unitId: resolvedUnitId,
        target: mapPickTarget ?? editingDraft.target,
        startSec,
        endSec,
        delayOffsetSec,
        delayMultiplier,
        stance,
        discipline,
        work,
      });
      onCancelEdit?.();
      return;
    }
    onCreateDraft({
      mode,
      taskKind,
      unitId: resolvedUnitId,
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

  return (
    <div className="composer-sheet" data-testid="directive-composer" data-ui-id="W12.BODY">
      <div className="row" style={{ justifyContent: 'space-between' }}>
        <span className="meta-note">
          {editingDraft ? `正在修改纸条 #${editingDraft.seq}` : '编写无头指令（可先不绑单位/标的物）'}
        </span>
        {editingDraft && onCancelEdit && (
          <button type="button" className="btn" onClick={onCancelEdit}>
            取消编辑
          </button>
        )}
      </div>

      <div className="row" data-ui-id="U2">
        <UiTag id="U2" />
        <select
          aria-label="指令形态"
          data-testid="select-mode"
          className="select"
          value={mode}
          onChange={(e) => setMode(e.target.value as DirectiveMode)}
        >
          <option data-ui-id="W5A.U2.opt_task" value="task">
            {optTxt('W5A.U2.opt_task', '单任务')}
          </option>
          <option data-ui-id="W5A.U2.opt_macro" value="macro">
            {optTxt('W5A.U2.opt_macro', '事件宏（前出→散兵坑→警戒）')}
          </option>
        </select>

        <span className="row" data-ui-id="U3" style={{ gap: '4px' }}>
          <UiTag id="U3" />
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
        </span>
      </div>

      <div className="row" data-ui-id="U4">
        <UiTag id="U4" />
        <label className="meta-note">
          起(s)
          <input
            type="number"
            data-testid="input-start-sec"
            className="input"
            style={{ width: '48px', marginLeft: '3px' }}
            value={startSec}
            min={0}
            onChange={(e) => setStartSec(Number(e.target.value))}
          />
        </label>
        <label className="meta-note">
          止(s)
          <input
            type="number"
            data-testid="input-end-sec"
            className="input"
            style={{ width: '48px', marginLeft: '3px' }}
            value={endSec}
            min={1}
            onChange={(e) => setEndSec(Number(e.target.value))}
          />
        </label>
        <label className="meta-note">
          偏移(s)
          <input
            type="number"
            data-testid="input-delay-offset"
            className="input"
            style={{ width: '46px', marginLeft: '3px' }}
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
            style={{ width: '46px', marginLeft: '3px' }}
            value={delayMultiplier}
            onChange={(e) => setDelayMultiplier(Number(e.target.value))}
          />
        </label>
      </div>

      <div className="row" data-ui-id="U5">
        <UiTag id="U5" />
        <select
          aria-label="预设执行单位"
          data-testid="composer-unit-select"
          className="select"
          disabled={bindSelectedUnit}
          value={explicitUnitId ?? ''}
          onChange={(e) => setExplicitUnitId(e.target.value ? Number(e.target.value) : undefined)}
        >
          <option value="">-- 无头（暂不绑单位） --</option>
          {friendlyUnits.map((u) => (
            <option key={u.id} value={u.id}>
              {u.name}
            </option>
          ))}
        </select>
        <label className="meta-note">
          <input
            type="checkbox"
            checked={bindSelectedUnit}
            onChange={(e) => setBindSelectedUnit(e.target.checked)}
          />{' '}
          自动跟选（{selectedUnitId ? `#${selectedUnitId}` : '无'}）
        </label>
      </div>

      <div className="row" style={{ justifyContent: 'space-between' }}>
        <div className="row">
          <button
            type="button"
            data-testid="btn-pick-target"
            data-ui-id="U6"
            className={isPickingTarget ? 'btn active' : 'btn'}
            onClick={() => onTogglePickTarget(!isPickingTarget)}
          >
            <UiTag id="U6" />
            {isPickingTarget ? '请点副地图选标...' : '去副地图拾取标的物'}
          </button>
          {mapPickTarget && (
            <button type="button" className="btn" onClick={onClearPickedTarget}>
              清除标({formatTarget(mapPickTarget)})
            </button>
          )}
        </div>

        <button
          type="button"
          data-testid="btn-create-draft"
          data-ui-id="U7"
          className="btn primary"
          onClick={handleSubmit}
        >
          <UiTag id="U7" />
          {editingDraft ? '保存改好纸条' : '撕下纸条送到手写板'}
        </button>
      </div>
    </div>
  );
};
