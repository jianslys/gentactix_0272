// GenTactix V2 frontend · 顶栏控制条（W1: T1..T14 / UI-01 & UI-02 照抄版）

import React, { useState } from 'react';
import { MAX_STEP_CACHE_HORIZON_TICKS, TICK_MS, type ScheduledStepEvent } from '../engine/types';
import type { MapViewMode } from '../render/canvasLayers';
import type { WeatherMode } from '../store/layerStore';
import { UiSelect } from './UiControls';
import { UiTag } from './UiTag';
import { WindowDockBar } from './WindowDockBar';

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
  mapViewMode?: MapViewMode;
  onToggleMapView?: () => void;
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
  mapViewMode = 'field',
  onToggleMapView,
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
  const [activeTab, setActiveTab] = useState<'situation' | 'plan' | 'intel'>('situation');
  const [showDesignNotes, setShowDesignNotes] = useState<boolean>(false);

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
    <div
      className="top-hud-bar"
      data-testid="top-control-bar"
      data-ui-id="W1"
    >
      {/* 顶栏层 1: UI-01 战略指令与品牌带 (1:1 照抄 concept-desktop.png) */}
      <div className="top-tier1" data-ui-id="UI-01">
        {/* 左侧：品牌 Logo 与军工标题 */}
        <div className="top-brand-group">
          <UiTag id="W1" kind="win" />
          <svg width="20" height="20" viewBox="0 0 18 18" style={{ flexShrink: 0 }}>
            <rect x="1" y="1" width="16" height="16" fill="none" stroke="#c89f55" strokeWidth="1.2" />
            <line x1="1" y1="17" x2="17" y2="1" stroke="#c89f55" strokeWidth="1.2" />
            <rect x="7" y="7" width="4" height="4" fill="#b84d43" />
          </svg>
          <div style={{ display: 'flex', flexDirection: 'column', justifyContent: 'center' }}>
            <span className="top-brand-title" style={{ fontSize: '13px', lineHeight: 1.1, letterSpacing: '0.14em' }}>
              GENTACTIX
            </span>
            <span
              className="top-brand-subtitle"
              style={{ fontSize: '8px', lineHeight: 1, letterSpacing: '0.16em', marginTop: '2px' }}
            >
              OPERATIONAL COMMAND
            </span>
          </div>
        </div>

        {/* 中部：战略三态导航页签 (战场态势 / 行动规划 / 情报分析) */}
        <div className="top-nav-tabs">
          <div
            className={`top-nav-tab ${activeTab === 'situation' ? 'active' : ''}`}
            onClick={() => setActiveTab('situation')}
          >
            战场态势
          </div>
          <div
            className={`top-nav-tab ${activeTab === 'plan' ? 'active' : ''}`}
            onClick={() => setActiveTab('plan')}
          >
            行动规划
          </div>
          <div
            className={`top-nav-tab ${activeTab === 'intel' ? 'active' : ''}`}
            onClick={() => setActiveTab('intel')}
          >
            情报分析
          </div>
        </div>

        {/* 右侧：DESIGN STUDY 标牌 + 状态绿灯 + 军规大时钟 + 设计说明 + ID 标牌 */}
        <div className="top-meta-group">
          <span className="top-meta-badge">DESIGN STUDY / 01</span>
          <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
            <span
              style={{
                display: 'inline-block',
                width: '6px',
                height: '6px',
                borderRadius: '50%',
                background: '#22c55e',
                boxShadow: '0 0 8px #22c55e',
              }}
            />
            <span className="top-clock">
              06:30:{(currentTick % 60).toString().padStart(2, '0')} LOCAL
            </span>
          </div>
          <button
            type="button"
            className="top-desc-btn"
            onClick={() => setShowDesignNotes(true)}
            title="查看设计说明 (UI-10)"
          >
            <span style={{ fontSize: '13px' }}>ⓘ</span>
            <span>设计说明</span>
          </button>
          <button
            type="button"
            data-testid="toggle-ui-ids"
            data-ui-id="T12"
            className={showUiIds ? 'btn active' : 'btn'}
            style={{ padding: '2px 6px', fontSize: '10px' }}
            onClick={onToggleUiIds}
          >
            <UiTag id="T12" />
            ID 标牌: {showUiIds ? '开' : '关'}
          </button>
        </div>
      </div>

      {/* 顶栏层 2: UI-02 战术环境与走表控制带 (1:1 照抄 concept-desktop.png) */}
      <div className="top-tier2" data-ui-id="UI-02">
        {/* 左侧：战役情报元数据 */}
        <div className="top-tier2-left">
          <span style={{ fontSize: '12px', fontWeight: 700, color: '#ffffff' }}>白桦行动</span>
          <span style={{ fontSize: '11px', color: '#c89f55', fontFamily: 'monospace', fontWeight: 600 }}>
            OP. BIRCH
          </span>
          <span className="top-divider">|</span>
          <span style={{ fontSize: '11px', color: '#8ba0ae', fontFamily: 'monospace' }}>
            22 JUN 1944
          </span>
          <span className="top-divider">|</span>
          <div style={{ display: 'flex', alignItems: 'center', gap: '2px' }}>
            <span style={{ fontSize: '11px', color: '#8ba0ae' }}>☁</span>
            <UiSelect
              uiId="T10"
              ariaLabel="L7天气"
              selectClassName="weather-select-concept"
              value={l7Weather}
              onChangeValue={(v) => onChangeWeather(v as WeatherMode)}
              options={[
                { value: 'fog', label: '薄云 18°C', optId: 'W1.T10.opt_fog' },
                { value: 'rain', label: '细雨 15°C', optId: 'W1.T10.opt_rain' },
                { value: 'snow', label: '飘雪 -2°C', optId: 'W1.T10.opt_snow' },
              ]}
            />
          </div>

          <span className="top-divider">|</span>

          {/* 回合与走表指示 */}
          <span data-ui-id="T1" style={{ display: 'inline-flex', alignItems: 'center', gap: '4px' }}>
            <UiTag id="T1" />
            <strong
              data-testid="current-tick"
              style={{ color: '#e2ddd1', fontSize: '11px', fontFamily: 'monospace' }}
            >
              当前回合 #{currentTick}
            </strong>
            <span
              data-testid="elapsed-sec"
              className="meta-note"
              style={{ fontSize: '10px', fontFamily: 'monospace', color: '#8ba0ae' }}
            >
              走表 {(elapsedMs / 1000).toFixed(1)}s / 16.0s
            </span>
          </span>
        </div>

        {/* 右侧：沙盘预演与走表操作 */}
        <div className="top-tier2-right">
          <span style={{ fontSize: '11px', color: '#6e8494', marginRight: '6px' }}>
            沙盘预演 · 虚拟战区
          </span>

          {/* 推进下一拍主按键 */}
          <button
            type="button"
            data-testid="btn-next-tick"
            data-ui-id="T2"
            className="btn-concept-step"
            onClick={onStepNext}
          >
            <UiTag id="T2" />
            推进 +16s
          </button>

          {/* 预演 / 暂停幽灵线框按钮 */}
          <button
            type="button"
            data-testid="btn-play-pause"
            data-ui-id="T3"
            className={`btn-concept-play ${playing ? 'active' : ''}`}
            disabled={!hasActivePlayback}
            onClick={onTogglePlay}
          >
            <UiTag id="T3" />
            {playing ? '⏸ 暂停' : '▷ 预演'}
          </button>

          {/* 1x 倍速下拉 */}
          <UiSelect
            uiId="T4"
            testId="select-playback-speed"
            ariaLabel="走表倍速"
            selectClassName="speed-select-concept"
            value={String(playbackSpeed)}
            onChangeValue={(v) => onChangeSpeed(Number(v) as PlaybackSpeed)}
            options={[
              { value: '1', label: '1x', optId: 'W1.T4.opt_1x' },
              { value: '2', label: '2x', optId: 'W1.T4.opt_2x' },
              { value: '4', label: '4x', optId: 'W1.T4.opt_4x' },
            ]}
          />

          {/* 洗带微型滑条 */}
          <span data-ui-id="T5" style={{ display: 'inline-flex', alignItems: 'center' }}>
            <UiTag id="T5" />
            <input
              type="range"
              data-testid="scrub-ms"
              min={0}
              max={TICK_MS}
              step={500}
              disabled={!hasActivePlayback}
              value={elapsedMs}
              style={{ width: '56px', height: '14px', accentColor: '#c89f55' }}
              onChange={(e) => onScrubMs(Number(e.target.value))}
            />
          </span>

          {/* 跳转 #T */}
          <span data-ui-id="T6" style={{ display: 'inline-flex', alignItems: 'center', gap: '2px' }}>
            <UiTag id="T6" />
            <input
              type="number"
              data-testid="jump-tick-input"
              className="input"
              style={{ width: '38px', padding: '1px 3px', fontSize: '11px', textAlign: 'center' }}
              min={currentTick}
              max={currentTick + MAX_STEP_CACHE_HORIZON_TICKS}
              value={jumpInput}
              onChange={(e) => setJumpInput(e.target.value)}
            />
            <button
              type="button"
              data-testid="btn-jump-tick"
              className="btn"
              style={{ padding: '2px 5px', fontSize: '11px' }}
              onClick={handleJump}
            >
              跳转
            </button>
          </span>

          {/* 快进至 #3 */}
          <button
            type="button"
            data-testid="btn-jump-3"
            data-ui-id="T7"
            className="btn"
            style={{ padding: '2px 5px', fontSize: '11px' }}
            onClick={() => onJumpToTick(3)}
          >
            <UiTag id="T7" />
            快进#3
          </button>

          {/* 特效与抽屉控制 */}
          <button
            type="button"
            data-testid="toggle-l7"
            data-ui-id="T9"
            className={l7Enabled ? 'btn active' : 'btn'}
            style={{ padding: '2px 5px', fontSize: '11px' }}
            onClick={onToggleL7}
          >
            <UiTag id="T9" />
            L7
          </button>
          <button
            type="button"
            data-testid="toggle-l9"
            data-ui-id="T11"
            className={l9Enabled ? 'btn active' : 'btn'}
            style={{ padding: '2px 5px', fontSize: '11px' }}
            onClick={onToggleL9}
          >
            <UiTag id="T11" />
            L9
          </button>
          <button
            type="button"
            data-testid="toggle-right-drawer"
            data-ui-id="T13"
            title="手写板 & 指挥抽屉 (快捷键: Shift+A)"
            className={rightCollapsed ? 'btn' : 'btn active'}
            style={{ padding: '2px 6px', fontSize: '11px' }}
            onClick={onToggleRightCollapsed}
          >
            <UiTag id="T13" />
            {rightCollapsed ? '手写板' : '收起'}
          </button>

          {onToggleMapView && (
            <button
              type="button"
              data-testid="top-toggle-map-view"
              data-ui-id="W1.MAP"
              className={mapViewMode === 'tactical' ? 'btn primary' : 'btn active'}
              style={{ padding: '2px 6px', fontSize: '11px' }}
              onClick={onToggleMapView}
              title="切换 主地图(近距现场) / 副地图(参谋军图) (快捷键: M)"
            >
              <UiTag id="W1.MAP" />
              {mapViewMode === 'field' ? '主图:现场' : '副图:参谋'}
            </button>
          )}

          {/* 窗口总坞栏 */}
          <WindowDockBar />
        </div>
      </div>

      {/* 书签栏（条件显示） */}
      {bookmarkTicks.length > 0 && (
        <div
          className="bookmark-row"
          style={{ padding: '2px 16px', background: 'rgba(9, 14, 18, 0.95)' }}
          data-testid="step-bookmarks"
          data-ui-id="T8"
        >
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

      {/* 前向预警横幅（条件显示） */}
      {lookaheadEvents.length > 0 && (
        <div
          className="remote-banner"
          style={{ margin: '0 16px', borderRadius: '0' }}
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

      {/* UI-10 设计说明模态框 (1:1 照抄 concept-design-notes.png) */}
      {showDesignNotes && (
        <div className="modal-overlay" onClick={() => setShowDesignNotes(false)}>
          <div
            className="modal-content"
            data-ui-id="UI-10"
            onClick={(e) => e.stopPropagation()}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
              <span className="ui-id-tag ui-id-win">UI-10</span>
              <span style={{ fontSize: '10px', color: '#6e8494', letterSpacing: '0.1em', textTransform: 'uppercase', fontFamily: 'monospace' }}>
                GENTACTIX / ART DIRECTION STUDY 01
              </span>
            </div>

            <div style={{ fontSize: '20px', fontWeight: 800, color: '#ffffff', letterSpacing: '0.04em' }}>
              印刷地图，精确指挥。
            </div>

            <div style={{ display: 'flex', flexDirection: 'column', gap: '10px', fontSize: '12px', lineHeight: 1.6, color: '#a0b4c0' }}>
              <p style={{ margin: 0 }}>
                底图来自 SD2 的视觉方向：暖灰纸色、低饱和道路、印刷式森林和等高线。没有厚重木桌、做旧污渍或装饰性立体棋子。
              </p>
              <p style={{ margin: 0 }}>
                兵牌采用 APP-6 风格的符号语法：蓝色矩形为己方，红色菱形为敌方；框内表示兵种，框上表示梯队。此页是美术示意，并非完整的 APP-6 标准实现。
              </p>
              <p style={{ margin: 0 }}>
                信息组织借鉴 CMO：单位详情集中在左侧，图层收于右下角，比例尺固定在屏幕底边。火控、路线和情报按需叠加。红色小字 UI-ID 为评审定位标记，不属于战场情报。
              </p>
            </div>

            {/* 核心五色板 */}
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(5, 1fr)', gap: '6px', margin: '8px 0' }}>
              <div style={{ height: '24px', background: '#e4dec8', border: '1px solid rgba(255,255,255,0.2)', borderRadius: '1px' }} title="#E4DEC8 暖灰纸色" />
              <div style={{ height: '24px', background: '#0f171d', border: '1px solid rgba(255,255,255,0.2)', borderRadius: '1px' }} title="#0F171D HUD 面板底色" />
              <div style={{ height: '24px', background: '#5b8296', border: '1px solid rgba(255,255,255,0.2)', borderRadius: '1px' }} title="#5B8296 北约友军蓝" />
              <div style={{ height: '24px', background: '#b84d43', border: '1px solid rgba(255,255,255,0.2)', borderRadius: '1px' }} title="#B84D43 敌军砖红" />
              <div style={{ height: '24px', background: '#c89f55', border: '1px solid rgba(255,255,255,0.2)', borderRadius: '1px' }} title="#C89F55 砂金强调色" />
            </div>

            <div style={{ fontSize: '11px', color: '#6e8494', lineHeight: 1.5 }}>
              可操作：选择兵牌、切换图层、拖动与缩放地图、切换情报视图、播放示意时钟。地图、兵力、路径和火控均为虚构静态样例，不读取或修改正在重构的后端。
            </div>

            <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: '6px' }}>
              <button
                type="button"
                className="btn primary"
                style={{ padding: '6px 14px', fontSize: '12px' }}
                onClick={() => setShowDesignNotes(false)}
              >
                返回作战地图
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
