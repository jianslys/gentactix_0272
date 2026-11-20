// GenTactix V2 frontend · 独立【指挥 UI】（与手写板 UIO 物理分离：绘制炮击区/冲锋箭头/许可 AO 并签发波次）

import React from 'react';
import type { CommanderWaveDraft, WaveDrawTool } from '../store/waveStore';

export interface CommanderWavePanelProps {
  draft: CommanderWaveDraft;
  onChangeTitle: (title: string) => void;
  onSelectTool: (tool: WaveDrawTool) => void;
  onCommitShape: () => void;
  onClearDraft: () => void;
  onIssueWave: () => void;
}

export const CommanderWavePanel: React.FC<CommanderWavePanelProps> = ({
  draft,
  onChangeTitle,
  onSelectTool,
  onCommitShape,
  onClearDraft,
  onIssueWave,
}) => {
  const readyCount =
    draft.geometry.barrages.length + draft.geometry.assaultArrows.length + draft.geometry.aos.length;

  return (
    <div className="panel" data-testid="commander-wave-panel" style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
      <div className="panel-header">
        <span>独立【指挥 UI】· 波次图例签发</span>
        <span className="meta-note">独立于 UIO · 签发后挂入顶栏回形针</span>
      </div>

      <input
        type="text"
        data-testid="wave-title-input"
        className="input"
        value={draft.title}
        onChange={(e) => onChangeTitle(e.target.value)}
        placeholder="输入波次名称"
      />

      <div className="row">
        <button
          type="button"
          data-testid="wave-tool-circle"
          className={draft.tool === 'barrage_circle' ? 'btn active' : 'btn'}
          onClick={() => onSelectTool(draft.tool === 'barrage_circle' ? 'none' : 'barrage_circle')}
        >
          绘预定炮击区(圆)
        </button>
        <button
          type="button"
          data-testid="wave-tool-poly"
          className={draft.tool === 'barrage_polygon' ? 'btn active' : 'btn'}
          onClick={() => onSelectTool(draft.tool === 'barrage_polygon' ? 'none' : 'barrage_polygon')}
        >
          绘炮击多边形
        </button>
        <button
          type="button"
          data-testid="wave-tool-arrow"
          className={draft.tool === 'assault_arrow' ? 'btn active' : 'btn'}
          onClick={() => onSelectTool(draft.tool === 'assault_arrow' ? 'none' : 'assault_arrow')}
        >
          绘冲锋箭头
        </button>
        <button
          type="button"
          data-testid="wave-tool-ao"
          className={draft.tool === 'ao_box' ? 'btn active' : 'btn'}
          onClick={() => onSelectTool(draft.tool === 'ao_box' ? 'none' : 'ao_box')}
        >
          绘许可 AO 框
        </button>
      </div>

      {draft.tool !== 'none' && (
        <div className="remote-banner">
          当前绘制：{draft.tool}（已在地图点选 {draft.pendingPoints.length} 点）。
          {(draft.tool === 'barrage_polygon' || draft.tool === 'assault_arrow') && (
            <button
              type="button"
              data-testid="wave-commit-shape"
              className="btn"
              style={{ marginLeft: '8px' }}
              onClick={onCommitShape}
            >
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
        <button type="button" className="btn" onClick={onClearDraft}>
          清空图例
        </button>
        <button
          type="button"
          data-testid="btn-issue-wave"
          className="btn primary"
          disabled={readyCount === 0 && draft.pendingPoints.length < 2}
          onClick={onIssueWave}
        >
          签发波次（生成 G 场纸条）
        </button>
      </div>
    </div>
  );
};
