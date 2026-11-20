// GenTactix V2 frontend · 顶栏控制条（W1: T1..T14）

import React, { useState } from 'react';
import { MAX_STEP_CACHE_HORIZON_TICKS, TICK_MS, type ScheduledStepEvent } from '../engine/types';
import type { WeatherMode } from '../store/layerStore';
import { UiTag } from './UiTag';

export type PlaybackSpeed = 1 | 2 | 4;

export interface TopControlBarProps {
  currentTick: number;
  elapsedMs: number;
  playing: boolean;
  playbackSpeed: PlaybackSpeed;
  hasActivePlayback: boolean;
  lookaheadEvents: ScheduledStepEvent[];
  scheduledEvents: ScheduledStepEvent[];
  l7Enabled: boolean;
  l7Weather: WeatherMode;
  l9Enabled: boolean;
  showUiIds: boolean;
  rightCollapsed: boolean;
  onStepNext: () => void;
  onTogglePlay: () => void;
  onChangeSpeed: (speed: PlaybackSpeed) => void;
  onScrubMs: (ms: number) => void;
  onJumpToTick: (targetTick: number) => void;
  onToggleL7: () => void;
  onChangeWeather: (w: WeatherMode) => void;
  onToggleL9: () => void;
  onToggleUiIds: () => void;
  onToggleRightCollapsed: () => void;
}

export const TopControlBar: React.FC<TopControlBarProps> = ({
  currentTick,
  elapsedMs,
  playing,
  playbackSpeed,
  hasActivePlayback,
  lookaheadEvents,
  scheduledEvents,
  l7Enabled,
  l7Weather,
  l9Enabled,
  showUiIds,
  rightCollapsed,
  onStepNext,
  onTogglePlay,
  onChangeSpeed,
  onScrubMs,
  onJumpToTick,
  onToggleL7,
  onChangeWeather,
  onToggleL9,
  onToggleUiIds,
  onToggleRightCollapsed,
}) => {
  const [jumpInput, setJumpInput] = useState<string>('3');

  const handleJump = () => {
    const n = Number(jumpInput);
    if (!Number.isFinite(n)) return;
    const clamped = Math.max(currentTick, Math.min(currentTick + MAX_STEP_CACHE_HORIZON_TICKS, Math.floor(n)));
    onJumpToTick(clamped);
  };

  const detonations = lookaheadEvents.filter((ev) => ev.kind === 'detonation');

  // 按目标回合聚合己方 60 天步长缓存关键节点书签
  const bookmarkTicks = Array.from(
    new Map<number, ScheduledStepEvent>(
      scheduledEvents
        .filter((ev) => ev.targetTick > currentTick)
        .map((ev) => [ev.targetTick, ev]),
    ).entries(),
  ).slice(0, 6);

  return (
    <div className="panel" style={{ width: '100%' }} data-testid="top-control-bar" data-ui-id="W1">
      <div className="row" style={{ justifyContent: 'space-between' }}>
        <div className="row">
          <span className="panel-tag">
            <UiTag id="W1" kind="win" />
            时钟 16s / 60 天步长缓存
          </span>

          <span data-ui-id="T1" className="row" style={{ gap: '4px' }}>
            <UiTag id="T1" />
            <strong data-testid="current-tick">当前回合 #{currentTick}</strong>
            <span data-testid="elapsed-sec" className="meta-note">
              走表 {(elapsedMs / 1000).toFixed(1)}s / 16.0s
            </span>
          </span>

          <button
            type="button"
            data-testid="btn-next-tick"
            data-ui-id="T2"
            className="btn primary"
            onClick={onStepNext}
          >
            <UiTag id="T2" />
            推进下一拍 (+16s)
          </button>

          <button
            type="button"
            data-testid="btn-play-pause"
            data-ui-id="T3"
            className={playing ? 'btn active' : 'btn'}
            disabled={!hasActivePlayback}
            onClick={onTogglePlay}
          >
            <UiTag id="T3" />
            {playing ? '暂停走表' : '逐秒播放'}
          </button>

          <select
            aria-label="走表倍速"
            data-testid="select-playback-speed"
            data-ui-id="T4"
            className="select"
            value={playbackSpeed}
            onChange={(e) => onChangeSpeed(Number(e.target.value) as PlaybackSpeed)}
          >
            <option value={1}>T4 1x 实时</option>
            <option value={2}>T4 2x 倍速</option>
            <option value={4}>T4 4x 快放</option>
          </select>

          <span className="row" data-ui-id="T5" style={{ gap: '2px' }}>
            <UiTag id="T5" />
            <input
              type="range"
              data-testid="scrub-ms"
              min={0}
              max={TICK_MS}
              step={500}
              disabled={!hasActivePlayback}
              value={elapsedMs}
              onChange={(e) => onScrubMs(Number(e.target.value))}
            />
          </span>

          <span className="row" data-ui-id="T6" style={{ gap: '4px' }}>
            <UiTag id="T6" />
            <input
              type="number"
              data-testid="jump-tick-input"
              className="input"
              style={{ width: '68px' }}
              min={currentTick}
              max={currentTick + MAX_STEP_CACHE_HORIZON_TICKS}
              value={jumpInput}
              onChange={(e) => setJumpInput(e.target.value)}
            />
            <button type="button" data-testid="btn-jump-tick" className="btn" onClick={handleJump}>
              跳转至 #T
            </button>
          </span>

          <button
            type="button"
            data-testid="btn-jump-3"
            data-ui-id="T7"
            className="btn"
            onClick={() => onJumpToTick(3)}
          >
            <UiTag id="T7" />
            快进至 #3
          </button>
        </div>

        <div className="row">
          <button
            type="button"
            data-testid="toggle-l7"
            data-ui-id="T9"
            className={l7Enabled ? 'btn active' : 'btn'}
            onClick={onToggleL7}
          >
            <UiTag id="T9" />
            L7 特效A：{l7Enabled ? '开' : '关'}
          </button>
          <span className="row" data-ui-id="T10" style={{ gap: '2px' }}>
            <UiTag id="T10" />
            <select
              aria-label="L7天气"
              className="select"
              value={l7Weather}
              onChange={(e) => onChangeWeather(e.target.value as WeatherMode)}
            >
              <option value="fog">薄雾</option>
              <option value="rain">细雨</option>
              <option value="snow">飘雪</option>
            </select>
          </span>
          <button
            type="button"
            data-testid="toggle-l9"
            data-ui-id="T11"
            className={l9Enabled ? 'btn active' : 'btn'}
            onClick={onToggleL9}
          >
            <UiTag id="T11" />
            L9 气氛B：{l9Enabled ? '开' : '关'}
          </button>
          <button
            type="button"
            data-testid="toggle-ui-ids"
            data-ui-id="T12"
            className={showUiIds ? 'btn active' : 'btn'}
            onClick={onToggleUiIds}
          >
            <UiTag id="T12" />
            ID 标牌：{showUiIds ? '开' : '关'}
          </button>
          <button
            type="button"
            data-testid="toggle-right-drawer"
            data-ui-id="T13"
            className={rightCollapsed ? 'btn' : 'btn active'}
            onClick={onToggleRightCollapsed}
          >
            <UiTag id="T13" />
            {rightCollapsed ? '展开手写板' : '收起手写板'}
          </button>
        </div>
      </div>

      {bookmarkTicks.length > 0 && (
        <div className="bookmark-row" data-testid="step-bookmarks" data-ui-id="T8">
          <span className="meta-note">
            <UiTag id="T8" />
            未来书签：
          </span>
          {bookmarkTicks.map(([tick, ev]) => {
            const jumpTarget = tick - 1 > currentTick ? tick - 1 : tick;
            return (
              <button
                key={ev.eventId}
                type="button"
                data-testid={`bookmark-tick-${tick}`}
                className="bookmark-pill"
                title={`点击跳转至 #${jumpTarget} 观察 #${tick} 事件前向预警`}
                onClick={() => onJumpToTick(jumpTarget)}
              >
                #{tick} {ev.kind === 'detonation' ? '💥爆破' : ev.kind === 'g_field_wave' ? '📜波次' : '📌指令'}
              </button>
            );
          })}
        </div>
      )}

      {lookaheadEvents.length > 0 && (
        <div
          className="remote-banner"
          style={{ marginTop: '6px' }}
          data-testid="lookahead-banner"
          data-ui-id="T14"
        >
          <UiTag id="T14" />
          {detonations.length > 0 ? (
            <strong>
              前向预警：下一回合 #{currentTick + 1} 定时炸药即将爆炸（
              {detonations
                .map(
                  (d) =>
                    `${(d.exactMs / 1000).toFixed(1)}s @ ${d.targetPos ? `${Math.round(d.targetPos.x)},${Math.round(d.targetPos.y)}` : '未知'}`,
                )
                .join('；')}
              ）
            </strong>
          ) : (
            <span>
              前向窥视 #{currentTick + 1}：己方预约事件 {lookaheadEvents.length} 项（
              {lookaheadEvents
                .map((e) => e.payload.label)
                .slice(0, 3)
                .join(' / ')}
              ）
            </span>
          )}
        </div>
      )}
    </div>
  );
};
