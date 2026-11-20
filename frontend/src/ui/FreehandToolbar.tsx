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
    <>
      {!embedded && (
        <span className="panel-tag">
          <UiTag id="W2" kind="win" />
          L6 手绘板（仅本地）
        </span>
      )}
      <button
        type="button"
        data-testid="l6-tool-none"
        data-ui-id="F1"
        className={tool === 'none' ? 'btn active' : 'btn'}
        onClick={() => onChangeTool('none')}
      >
        <UiTag id="F1" />
        鼠标选兵
      </button>
      <button
        type="button"
        data-testid="l6-tool-pen"
        data-ui-id="F2"
        className={tool === 'pen' ? 'btn active' : 'btn'}
        onClick={() => onChangeTool('pen')}
      >
        <UiTag id="F2" />
        手绘笔
      </button>
      <button
        type="button"
        data-testid="l6-tool-eraser"
        data-ui-id="F3"
        className={tool === 'eraser' ? 'btn active' : 'btn'}
        onClick={() => onChangeTool('eraser')}
      >
        <UiTag id="F3" />
        橡皮擦
      </button>
      <div className="color-swatches" data-ui-id="F4">
        <UiTag id="F4" />
        {COLORS.map((c) => (
          <button
            key={c.value}
            type="button"
            data-ui-id={c.optId}
            title={`${c.optId} · ${c.label}`}
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
      <button
        type="button"
        data-testid="l6-clear"
        data-ui-id="F5"
        className="btn danger"
        onClick={onClear}
      >
        <UiTag id="F5" />
        清空画线
      </button>
      <span className="meta-note" data-testid="l6-count" data-ui-id="F6">
        <UiTag id="F6" />
        笔画 {strokeCount} · {storageBackend === 'indexeddb' ? 'IndexedDB 已同步' : 'LocalStorage 已同步'}
      </span>
    </>
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
