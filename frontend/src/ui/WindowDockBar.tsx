import React from 'react';
import { useGameOptional } from '../store/gameContext';
import { MANAGED_WINDOW_IDS } from '../store/windowStore';
import { UiTag } from './UiTag';

/**
 * 顶部 W1 浮动窗口总开关坞栏（Window Dock Bar）：
 * - 一键开启 / 隐藏任意面板（W2..W8）
 * - 即使把窗口全部拖乱或关掉，点一下 `重置布局`（W1.dock.reset）立刻恢复出厂排列。
 */
export const WindowDockBar: React.FC = () => {
  const game = useGameOptional();
  if (!game) return null;

  return (
    <div className="top-group dock-bar" data-ui-id="W1.dock" data-testid="window-dock-bar">
      {MANAGED_WINDOW_IDS.map((winId) => {
        const w = game.windows[winId];
        const dockUiId = `W1.dock.${winId}`;
        return (
          <button
            key={winId}
            type="button"
            className={`btn dock-pill ${w.open ? 'active' : ''}`}
            data-ui-id={dockUiId}
            data-testid={`dock-toggle-${winId}`}
            title={`${w.open ? '关闭' : '打开'} ${w.title}（${w.sourceFile}）`}
            onClick={() => game.toggleWindow(winId)}
          >
            <UiTag id={dockUiId} kind="ctl" />
            {w.shortLabel}
          </button>
        );
      })}
      <button
        type="button"
        className="btn"
        data-ui-id="W1.dock.reset"
        data-testid="dock-reset-windows"
        title="恢复所有浮动窗口默认位置并清空本地布局缓存"
        onClick={game.resetWindows}
      >
        <UiTag id="W1.dock.reset" kind="ctl" />
        重置布局
      </button>
    </div>
  );
};
