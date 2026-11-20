// GenTactix V2 frontend · 独立【指挥 UI】（W5B: C1..C9，支持拖圆定半径/橡皮筋预览/单步撤销/Esc退出）

import React from 'react';
import type { CommanderWaveDraft, WaveDrawTool } from '../store/waveStore';
import { UiTag } from './UiTag';

export interface CommanderWavePanelProps {
  draft: CommanderWaveDraft;
  onChangeTitle: (title: string) => void;
  onSelectTool: (tool: WaveDrawTool) => void;
  onCommitShape: () => void;
  onUndoPoint: () => void;
  onClearDraft: () => void;
  onIssueWave: () => void;
}

export const CommanderWavePanel: React.FC<CommanderWavePanelProps> = ({
  draft,
  onChangeTitle,
  onSelectTool,
  onCommitShape,
  onUndoPoint,
  onClearDraft,
  onIssueWave,
}) => {
  const readyCount =
    draft.geometry.barrages.length + draft.geometry.assaultArrows.length + draft.geometry.aos.length;

  return (
    <div
      className="panel"
      data-testid="commander-wave-panel"
      data-ui-id="W5B"
      style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}
    >
      <div className="panel-header">
        <span>
          <UiTag id="W5B" kind="win" />
          独立【指挥 UI】· 波次图例签发
        </span>
        <span className="meta-note">独立于 UIO · 签发后挂入顶栏回形针</span>
      </div>

      <div className="row" data-ui-id="C1">
        <UiTag id="C1" />
        <input
          type="text"
          data-testid="wave-title-input"
          className="input"
          style={{ flex: 1 }}
          value={draft.title}
          onChange={(e) => onChangeTitle(e.target.value)}
          placeholder="输入波次名称"
        />
      </div>

      <div className="row">
        <button
          type="button"
          data-testid="wave-tool-circle"
          data-ui-id="C2"
          className={draft.tool === 'barrage_circle' ? 'btn active' : 'btn'}
          onClick={() => onSelectTool(draft.tool === 'barrage_circle' ? 'none' : 'barrage_circle')}
        >
          <UiTag id="C2" />
          绘预定炮击区(拖圆)
        </button>
        <button
          type="button"
          data-testid="wave-tool-poly"
          data-ui-id="C3"
          className={draft.tool === 'barrage_polygon' ? 'btn active' : 'btn'}
          onClick={() => onSelectTool(draft.tool === 'barrage_polygon' ? 'none' : 'barrage_polygon')}
        >
          <UiTag id="C3" />
          绘炮击多边形
        </button>
        <button
          type="button"
          data-testid="wave-tool-arrow"
          data-ui-id="C4"
          className={draft.tool === 'assault_arrow' ? 'btn active' : 'btn'}
          onClick={() => onSelectTool(draft.tool === 'assault_arrow' ? 'none' : 'assault_arrow')}
        >
          <UiTag id="C4" />
          绘冲锋箭头
        </button>
        <button
          type="button"
          data-testid="wave-tool-ao"
          data-ui-id="C5"
          className={draft.tool === 'ao_box' ? 'btn active' : 'btn'}
          onClick={() => onSelectTool(draft.tool === 'ao_box' ? 'none' : 'ao_box')}
        >
          <UiTag id="C5" />
          绘许可 AO 框
        </button>
      </div>

      {draft.tool !== 'none' && (
        <div className="remote-banner">
          {draft.tool === 'barrage_circle'
            ? '在地图上按住左键拖拽可实时拉出炮击半径（单点默认 42m），Esc 退出。'
            : `当前绘制：${draft.tool}（已点 ${draft.pendingPoints.length} 点，移动鼠标可见虚线橡皮筋，Esc 退出）。`}
          {(draft.tool === 'barrage_polygon' || draft.tool === 'assault_arrow') && (
            <button
              type="button"
              data-testid="wave-commit-shape"
              data-ui-id="C6"
              className="btn"
              style={{ marginLeft: '8px' }}
              onClick={onCommitShape}
            >
              <UiTag id="C6" />
              闭合/完成本图元
            </button>
          )}
        </div>
      )}

      <div className="meta-note" data-testid="wave-draft-summary">
        已绘图元：炮击区 {draft.geometry.barrages.length} · 冲锋箭头 {draft.geometry.assaultArrows.length} · 许可 AO{' '}
        {draft.geometry.aos.length}
      </div>

      <div className="row" style={{ justifyContent: 'space-between' }}>
        <div className="row">
          <button
            type="button"
            data-testid="wave-undo-point"
            data-ui-id="C7"
            className="btn"
            disabled={draft.pendingPoints.length === 0 && readyCount === 0}
            onClick={onUndoPoint}
          >
            <UiTag id="C7" />
            撤销上一点
          </button>
          <button
            type="button"
            data-testid="wave-clear-draft"
            data-ui-id="C8"
            className="btn"
            onClick={onClearDraft}
          >
            <UiTag id="C8" />
            清空图例
          </button>
        </div>
        <button
          type="button"
          data-testid="btn-issue-wave"
          data-ui-id="C9"
          className="btn primary"
          disabled={readyCount === 0 && draft.pendingPoints.length < 2}
          onClick={onIssueWave}
        >
          <UiTag id="C9" />
          签发波次（生成 G 场纸条）
        </button>
      </div>
    </div>
  );
};
