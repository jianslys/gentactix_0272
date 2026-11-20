// GenTactix V2 frontend · 顶栏控制条（16s 逐秒走表、#0..#324000 步长跳转、己方前向预警、L7/L9 开关）

import React, { useState } from 'react';
import { MAX_STEP_CACHE_HORIZON_TICKS, TICK_MS, type ScheduledStepEvent } from '../engine/types';
import type { WeatherMode } from '../store/layerStore';

export interface TopControlBarProps {
  currentTick: number;
  elapsedMs: number;
  playing: boolean;
  hasActivePlayback: boolean;
  lookaheadEvents: ScheduledStepEvent[];
  l7Enabled: boolean;
  l7Weather: WeatherMode;
  l9Enabled: boolean;
  onStepNext: () => void;
  onTogglePlay: () => void;
  onScrubMs: (ms: number) => void;
  onJumpToTick: (targetTick: number) => void;
  onToggleL7: () => void;
  onChangeWeather: (w: WeatherMode) => void;
  onToggleL9: () => void;
}

export const TopControlBar: React.FC<TopControlBarProps> = ({
  currentTick,
  elapsedMs,
  playing,
  hasActivePlayback,
  lookaheadEvents,
  l7Enabled,
  l7Weather,
  l9Enabled,
  onStepNext,
  onTogglePlay,
  onScrubMs,
  onJumpToTick,
  onToggleL7,
  onChangeWeather,
  onToggleL9,
}) => {
  const [jumpInput, setJumpInput] = useState<string>('3');

  const handleJump = () => {
    const n = Number(jumpInput);
    if (!Number.isFinite(n)) return;
    const clamped = Math.max(currentTick, Math.min(currentTick + MAX_STEP_CACHE_HORIZON_TICKS, Math.floor(n)));
    onJumpToTick(clamped);
  };

  const detonations = lookaheadEvents.filter((ev) => ev.kind === 'detonation');

  return (
    <div className="panel" style={{ width: '100%' }} data-testid="top-control-bar">
      <div className="row" style={{ justifyContent: 'space-between' }}>
        <div className="row">
          <span className="panel-tag">时钟 16s / 60 天步长缓存</span>
          <strong data-testid="current-tick">当前回合 #{currentTick}</strong>
          <span data-testid="elapsed-sec" className="meta-note">
            走表 {(elapsedMs / 1000).toFixed(1)}s / 16.0s
          </span>

          <button type="button" data-testid="btn-next-tick" className="btn primary" onClick={onStepNext}>
            推进下一拍 (+16s)
          </button>

          <button
            type="button"
            data-testid="btn-play-pause"
            className={playing ? 'btn active' : 'btn'}
            disabled={!hasActivePlayback}
            onClick={onTogglePlay}
          >
            {playing ? '暂停走表' : '逐秒播放'}
          </button>

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

          <input
            type="number"
            data-testid="jump-tick-input"
            className="input"
            style={{ width: '76px' }}
            min={currentTick}
            max={currentTick + MAX_STEP_CACHE_HORIZON_TICKS}
            value={jumpInput}
            onChange={(e) => setJumpInput(e.target.value)}
          />
          <button type="button" data-testid="btn-jump-tick" className="btn" onClick={handleJump}>
            跳转至 #T
          </button>
          <button type="button" data-testid="btn-jump-3" className="btn" onClick={() => onJumpToTick(3)}>
            快进至 #3
          </button>
        </div>

        <div className="row">
          <button
            type="button"
            data-testid="toggle-l7"
            className={l7Enabled ? 'btn active' : 'btn'}
            onClick={onToggleL7}
          >
            L7 特效A：{l7Enabled ? '开' : '关'}
          </button>
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
          <button
            type="button"
            data-testid="toggle-l9"
            className={l9Enabled ? 'btn active' : 'btn'}
            onClick={onToggleL9}
          >
            L9 气氛B：{l9Enabled ? '开' : '关'}
          </button>
        </div>
      </div>

      {lookaheadEvents.length > 0 && (
        <div
          className="remote-banner"
          style={{ marginTop: '6px' }}
          data-testid="lookahead-banner"
        >
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
                .map((e) => (e.payload.type === 'g_field_wave' ? e.payload.label : e.payload.label))
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
