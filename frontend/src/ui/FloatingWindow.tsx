import React, { useEffect, useRef } from 'react';
import type { ManagedWindowId, WindowState } from '../store/windowStore';
import { UiTag } from './UiTag';

export interface FloatingWindowProps {
  id: ManagedWindowId;
  title?: string;
  sourceFile?: string;
  testId?: string;
  className?: string;
  winState: WindowState;
  onFocus: (id: ManagedWindowId) => void;
  onMove: (id: ManagedWindowId, x: number, y: number) => void;
  onToggleMinimize: (id: ManagedWindowId) => void;
  onClose: (id: ManagedWindowId) => void;
  children: React.ReactNode;
}

/**
 * 通用 Docked-by-Default -> Detach-on-Drag 浮动窗口外壳：
 * - 默认随 `.hud-left` / `.hud-right` 自适应流式排布，互不重叠；
 * - 按住标题栏拖动立即分离成自由绝对定位窗口（记录至 localStorage），且限制不遮挡 W1 顶栏；
 * - 标题栏显示窗口级 <UiTag id={id} kind="win" />、最小化（${id}.btn_min）与关闭（${id}.btn_close）。
 */
export const FloatingWindow: React.FC<FloatingWindowProps> = ({
  id,
  title,
  sourceFile,
  testId,
  className,
  winState,
  onFocus,
  onMove,
  onToggleMinimize,
  onClose,
  children,
}) => {
  const winRef = useRef<HTMLElement | null>(null);
  const dragRef = useRef<{
    startClientX: number;
    startClientY: number;
    origX: number;
    origY: number;
  } | null>(null);

  useEffect(() => {
    const handlePointerMove = (e: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      const vw = typeof window !== 'undefined' && window.innerWidth > 0 ? window.innerWidth : 1280;
      const vh = typeof window !== 'undefined' && window.innerHeight > 0 ? window.innerHeight : 800;
      const rawX = drag.origX + (e.clientX - drag.startClientX);
      const rawY = drag.origY + (e.clientY - drag.startClientY);
      const clampedX = Math.max(0, Math.min(Math.max(0, vw - 140), rawX));
      const clampedY = Math.max(42, Math.min(Math.max(42, vh - 36), rawY));
      onMove(id, clampedX, clampedY);
    };
    const handlePointerUp = () => {
      dragRef.current = null;
    };
    window.addEventListener('pointermove', handlePointerMove);
    window.addEventListener('pointerup', handlePointerUp);
    window.addEventListener('pointercancel', handlePointerUp);
    return () => {
      window.removeEventListener('pointermove', handlePointerMove);
      window.removeEventListener('pointerup', handlePointerUp);
      window.removeEventListener('pointercancel', handlePointerUp);
    };
  }, [id, onMove]);

  if (!winState.open) return null;

  const resolvedTitle = title ?? winState.title;
  const resolvedSource = sourceFile ?? winState.sourceFile;

  const detachedStyle: React.CSSProperties | undefined = winState.floating
    ? {
        position: 'absolute',
        zIndex: winState.zIndex,
        width: winState.width,
        left: winState.x,
        top: winState.y,
      }
    : undefined;

  const handleHeaderPointerDown = (e: React.PointerEvent<HTMLElement>) => {
    const target = e.target as HTMLElement | null;
    if (target?.closest('.floating-win-actions')) return;
    onFocus(id);

    const el = winRef.current;
    let origX = winState.x;
    let origY = winState.y;
    if (el) {
      const rect = el.getBoundingClientRect();
      if (rect.width > 0) {
        const shellEl = el.closest('.hud-shell') as HTMLElement | null;
        const shellRect = shellEl?.getBoundingClientRect();
        if (shellRect && shellRect.width > 0) {
          origX = rect.left - shellRect.left;
          origY = rect.top - shellRect.top;
        } else {
          origX = rect.left;
          origY = rect.top;
        }
      }
    }
    dragRef.current = {
      startClientX: e.clientX,
      startClientY: e.clientY,
      origX,
      origY,
    };
    (e.currentTarget as HTMLElement & { setPointerCapture?: (pid: number) => void }).setPointerCapture?.(
      e.pointerId,
    );
  };

  return (
    <section
      ref={winRef}
      className={`floating-window ${winState.floating ? 'floating-detached' : 'floating-docked'} panel ${
        winState.minimized ? 'floating-window-min' : ''
      } ${className ?? ''}`}
      style={detachedStyle}
      data-ui-id={id}
      data-testid={testId}
      data-window-id={id}
      data-window-minimized={winState.minimized ? 'true' : 'false'}
      onPointerDown={() => onFocus(id)}
    >
      <header
        className="floating-win-header"
        data-testid={`win-drag-${id}`}
        onPointerDown={handleHeaderPointerDown}
        onDoubleClick={() => onToggleMinimize(id)}
        title={`按住拖出浮动 · 双击折叠（${resolvedSource}）`}
      >
        <div className="floating-win-title">
          <UiTag id={id} kind="win" source={resolvedSource} />
          <span>{resolvedTitle}</span>
        </div>
        <div className="floating-win-actions">
          <button
            type="button"
            className="win-btn"
            data-ui-id={`${id}.btn_min`}
            data-testid={`win-min-${id}`}
            title={winState.minimized ? '展开窗口' : '最小化窗口'}
            onClick={() => onToggleMinimize(id)}
          >
            <UiTag id={`${id}.btn_min`} kind="ctl" />
            {winState.minimized ? '▢' : '—'}
          </button>
          <button
            type="button"
            className="win-btn win-btn-close"
            data-ui-id={`${id}.btn_close`}
            data-testid={`win-close-${id}`}
            title="关闭窗口（可在顶部坞栏重新打开）"
            onClick={() => onClose(id)}
          >
            <UiTag id={`${id}.btn_close`} kind="ctl" />
            ×
          </button>
        </div>
      </header>
      {!winState.minimized ? <div className="floating-win-body">{children}</div> : null}
    </section>
  );
};
