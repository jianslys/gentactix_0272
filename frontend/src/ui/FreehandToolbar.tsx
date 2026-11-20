// GenTactix V2 frontend · Layer 6 手绘工具条（W2: F1..F6）

import React from 'react';
import type { FreehandTool } from '../store/layerStore';
import { UiTag } from './UiTag';

export interface FreehandToolbarProps {
  tool: FreehandTool;
  color: string;
  strokeCount: number;
  storageBackend: 'indexeddb' | 'localstorage';
  embedded?: boolean;
  onChangeTool: (tool: FreehandTool) => void;
  onChangeColor: (color: string) => void;
  onClear: () => void;
}

const COLORS = [
  { value: '#dc2626', label: '红铅', optId: 'W2.F4.opt_red' },
  { value: '#2563eb', label: '蓝铅', optId: 'W2.F4.opt_blue' },
  { value: '#15803d', label: '绿铅', optId: 'W2.F4.opt_green' },
  { value: '#1c1917', label: '炭黑', optId: 'W2.F4.opt_black' },
];

export const FreehandToolbar: React.FC<FreehandToolbarProps> = ({
  tool,
  color,
  strokeCount,
  storageBackend,
  embedded = false,
  onChangeTool,
  onChangeColor,
  onClear,
}) => {
  const inner = (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '6px', width: '100%' }}>
      {!embedded && (
        <span className="panel-tag">
          <UiTag id="W2" kind="win" />
          L6 手绘板（仅本地）
        </span>
      )}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: '4px' }}>
        <button
          type="button"
          data-testid="l6-tool-none"
          data-ui-id="F1"
          className={tool === 'none' ? 'btn active' : 'btn'}
          style={{ padding: '3px 4px', fontSize: '11px', textAlign: 'center' }}
          onClick={() => onChangeTool('none')}
        >
          <UiTag id="F1" />
          选兵
        </button>
        <button
          type="button"
          data-testid="l6-tool-pen"
          data-ui-id="F2"
          className={tool === 'pen' ? 'btn active' : 'btn'}
          style={{ padding: '3px 4px', fontSize: '11px', textAlign: 'center' }}
          onClick={() => onChangeTool('pen')}
        >
          <UiTag id="F2" />
          画笔
        </button>
        <button
          type="button"
          data-testid="l6-tool-eraser"
          data-ui-id="F3"
          className={tool === 'eraser' ? 'btn active' : 'btn'}
          style={{ padding: '3px 4px', fontSize: '11px', textAlign: 'center' }}
          onClick={() => onChangeTool('eraser')}
        >
          <UiTag id="F3" />
          橡皮
        </button>
        <button
          type="button"
          data-testid="l6-clear"
          data-ui-id="F5"
          className="btn danger"
          style={{ padding: '3px 4px', fontSize: '11px', textAlign: 'center' }}
          onClick={onClear}
        >
          <UiTag id="F5" />
          清空
        </button>
      </div>

      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', borderTop: '1px solid rgba(82, 108, 124, 0.25)', paddingTop: '4px' }}>
        <div className="color-swatches" data-ui-id="F4" style={{ display: 'flex', gap: '5px' }}>
          <UiTag id="F4" />
          {COLORS.map((c) => (
            <button
              key={c.value}
              type="button"
              data-ui-id={c.optId}
              title={`${c.optId} · ${c.label}`}
              aria-label={c.label}
              className={color === c.value ? 'swatch active' : 'swatch'}
              style={{ background: c.value, width: '16px', height: '16px' }}
              onClick={() => {
                onChangeColor(c.value);
                if (tool === 'none') onChangeTool('pen');
              }}
            />
          ))}
        </div>
        <span className="meta-note" data-testid="l6-count" data-ui-id="F6" style={{ fontSize: '10px', color: '#7b8f9f' }}>
          <UiTag id="F6" />
          笔画 {strokeCount} · {storageBackend === 'indexeddb' ? 'IDB' : 'Local'}
        </span>
      </div>
    </div>
  );

  if (embedded) {
    return <div className="freehand-bar">{inner}</div>;
  }
  return (
    <div className="panel freehand-bar" data-testid="freehand-toolbar" data-ui-id="W2">
      {inner}
    </div>
  );
};
