// GenTactix V2 frontend · Layer 6 手绘工具条（画笔 / 颜色 / 橡皮 / 清空 / 本地持久化提示）

import React from 'react';
import type { FreehandTool } from '../store/layerStore';

export interface FreehandToolbarProps {
  tool: FreehandTool;
  color: string;
  strokeCount: number;
  storageBackend: 'indexeddb' | 'localstorage';
  onChangeTool: (tool: FreehandTool) => void;
  onChangeColor: (color: string) => void;
  onClear: () => void;
}

const COLORS = [
  { value: '#dc2626', label: '红铅' },
  { value: '#2563eb', label: '蓝铅' },
  { value: '#15803d', label: '绿铅' },
  { value: '#1c1917', label: '炭黑' },
];

export const FreehandToolbar: React.FC<FreehandToolbarProps> = ({
  tool,
  color,
  strokeCount,
  storageBackend,
  onChangeTool,
  onChangeColor,
  onClear,
}) => {
  return (
    <div className="panel freehand-bar" data-testid="freehand-toolbar">
      <span className="panel-tag">L6 手绘板（仅本地）</span>
      <button
        type="button"
        data-testid="l6-tool-none"
        className={tool === 'none' ? 'btn active' : 'btn'}
        onClick={() => onChangeTool('none')}
      >
        鼠标选兵
      </button>
      <button
        type="button"
        data-testid="l6-tool-pen"
        className={tool === 'pen' ? 'btn active' : 'btn'}
        onClick={() => onChangeTool('pen')}
      >
        手绘笔
      </button>
      <button
        type="button"
        data-testid="l6-tool-eraser"
        className={tool === 'eraser' ? 'btn active' : 'btn'}
        onClick={() => onChangeTool('eraser')}
      >
        橡皮擦
      </button>
      <div className="color-swatches">
        {COLORS.map((c) => (
          <button
            key={c.value}
            type="button"
            title={c.label}
            aria-label={c.label}
            className={color === c.value ? 'swatch active' : 'swatch'}
            style={{ background: c.value }}
            onClick={() => {
              onChangeColor(c.value);
              if (tool === 'none') onChangeTool('pen');
            }}
          />
        ))}
      </div>
      <button type="button" data-testid="l6-clear" className="btn danger" onClick={onClear}>
        清空画线
      </button>
      <span className="meta-note" data-testid="l6-count">
        笔画 {strokeCount} · {storageBackend === 'indexeddb' ? 'IndexedDB 已同步' : 'LocalStorage 已同步'}
      </span>
    </div>
  );
};
