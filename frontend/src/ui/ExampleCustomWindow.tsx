import React from 'react';
import { useGame } from '../store/gameContext';
import { UiButton } from './UiControls';
import { UiTag } from './UiTag';

/**
 * W8 · 复制即用的前端程序员傻瓜开发模板窗口（Example Custom Window Template）
 *
 * 怎么自己加一个新面板？只需三步：
 * 1. 复制本文件改名为你的组件（例如 ArtillerySupportWindow.tsx）；
 * 2. 在 `src/store/windowStore.ts` 加一个窗口 ID（例如 `'W9'`）；
 * 3. 在本文件里用 `const game = useGame();` 拿数据、调 `game.sendQuickCommand(...)` 下令。
 */
export const ExampleCustomWindow: React.FC = () => {
  const game = useGame();
  const unit = game.selectedUnit ?? game.displaySnapshot?.units[0] ?? null;

  return (
    <div className="example-custom-win" data-testid="example-custom-window">
      <div className="card">
        <div className="row">
          <UiTag id="W8.S1" />
          <strong>实时读世界态（零 Props 传参）</strong>
        </div>
        <div className="meta-note">
          当前步长 Tick #{game.tickNumber} · 走表 {(game.elapsedMs / 1000).toFixed(1)}s / 16.0s · 数据源：
          {game.backendMode}
        </div>
        <div className="meta-note">
          当前选中单位：{unit ? `${unit.name} (${Math.round(unit.pos.x)}m, ${Math.round(unit.pos.y)}m)` : '未选中'}
        </div>
      </div>

      <div className="row">
        <UiButton
          uiId="W8.opt_hold_selected"
          className="btn"
          disabled={!unit}
          onClick={() => {
            if (!unit) return;
            void game.sendQuickCommand({ kind: 'hold_unit', unit_id: unit.id });
          }}
        >
          就地固守 (hold_unit)
        </UiButton>

        <UiButton
          uiId="W8.opt_step_next"
          className="btn primary"
          onClick={() => void game.stepNext()}
        >
          推进下一 16s 步长
        </UiButton>
      </div>

      {game.lastCommandOutcome ? (
        <div className="meta-note" data-testid="example-cmd-outcome">
          回执：[{game.lastCommandOutcome.code}] {game.lastCommandOutcome.detail}
        </div>
      ) : null}
    </div>
  );
};
