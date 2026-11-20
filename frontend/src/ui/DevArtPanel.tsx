import React from 'react';
import { getArtSlotStatuses } from '../art/assetLoader';
import { ART_SLOT_LIST } from '../art/assetManifest';
import type { UnitDetailView } from '../engine/types';
import { useGame } from '../store/gameContext';
import { UiTag } from './UiTag';

export const SAMPLE_SOLDIER_DETAIL: UnitDetailView = {
  id: 9001,
  callsign: '样板一班（4部位受伤演示）',
  team: 'red',
  role: 'rifle_squad',
  pos: { x: 188, y: 248 },
  facing_deg: 25,
  alive: true,
  vis_tier: 'full',
  men: 8,
  hp: 74,
  suppression: 0.34,
  posture: 'active',
  weapon_id: 'rifle_556',
  ammo_rounds: 640,
  soldiers: [
    {
      soldier_id: 1,
      alive: true,
      hp: 86,
      max_hp: 100,
      regions: [
        { integrity: 1.0, cumulative_damage: 0 },
        { integrity: 0.92, cumulative_damage: 8 },
        { integrity: 0.72, cumulative_damage: 28 },
        { integrity: 1.0, cumulative_damage: 0 },
      ],
      bleeding_loss: 1.5,
      bleed_rate_per_tick: 0.4,
      suppression: 0.25,
    },
    {
      soldier_id: 2,
      alive: true,
      hp: 62,
      max_hp: 100,
      regions: [
        { integrity: 0.85, cumulative_damage: 15 },
        { integrity: 0.65, cumulative_damage: 35 },
        { integrity: 0.58, cumulative_damage: 42 },
        { integrity: 0.41, cumulative_damage: 59 },
      ],
      bleeding_loss: 6.2,
      bleed_rate_per_tick: 1.8,
      suppression: 0.52,
    },
  ],
};

export const SAMPLE_VEHICLE_DETAIL: UnitDetailView = {
  id: 9002,
  callsign: '样板ZTZ履带车（8部件+RHA装甲演示）',
  team: 'red',
  role: 'armored_vehicle',
  pos: { x: 240, y: 220 },
  facing_deg: 18,
  alive: true,
  vis_tier: 'full',
  is_vehicle: true,
  hp: 420,
  suppression: 0.12,
  weapon_id: 'tank_cannon_105',
  ammo_rounds: 34,
  vehicle_damage: {
    armor_rha: { front_mm: 220, side_mm: 85, rear_mm: 40, top_mm: 28 },
    subsystems: [0.95, 0.48, 1.0, 0.82, 0.65, 1.0, 0.91, 0.88],
    crew_alive: 4,
    crew_max: 4,
    ammo_cooked_off: false,
    burning: false,
  },
};

/**
 * W7 · 保姆级美术与真后端联调工作台（默认收起，点顶部 W1.dock.W7 随开随用）。
 * 完全通过 useGame() 读取上下文，演示零 Props 透传的开发模式。
 */
export const DevArtPanel: React.FC = () => {
  const game = useGame();
  const slotStatuses = getArtSlotStatuses();

  return (
    <div className="dev-art-panel" data-testid="dev-art-panel">
      <div className="lane">
        <div className="lane-title">
          <span>引擎数据源与连通性</span>
          <span className="meta-note">{game.backendMode === 'http' ? '真后端 HTTP (:8767)' : '离线 Mock 沙盒'}</span>
        </div>
        <div className="row" data-ui-id="W7.B1">
          <UiTag id="W7.B1" />
          <button
            type="button"
            className={`btn ${game.backendMode === 'mock' ? 'active' : ''}`}
            data-ui-id="W7.B1.opt_mock"
            data-testid="btn-mode-mock"
            onClick={() => game.setBackendMode('mock')}
          >
            <UiTag id="W7.B1.opt_mock" kind="opt" />
            Mock 沙盒
          </button>
          <button
            type="button"
            className={`btn ${game.backendMode === 'http' ? 'active' : ''}`}
            data-ui-id="W7.B1.opt_http"
            data-testid="btn-mode-http"
            onClick={() => game.setBackendMode('http')}
          >
            <UiTag id="W7.B1.opt_http" kind="opt" />
            HTTP 真后端
          </button>
        </div>
        <div className="row" data-ui-id="W7.B2">
          <UiTag id="W7.B2" />
          <input
            type="text"
            className="input"
            style={{ flex: 1 }}
            placeholder="留空走 Vite 代理 /api，或填 http://127.0.0.1:8767"
            value={game.httpBaseUrl}
            onChange={(e) => game.setHttpBaseUrl(e.target.value)}
          />
          <button
            type="button"
            className="btn"
            data-ui-id="W7.B3"
            data-testid="btn-check-health"
            onClick={() => void game.checkBackendHealth()}
          >
            <UiTag id="W7.B3" />
            测连通 /health
          </button>
        </div>
        {game.healthInfo ? (
          <div className="meta-note" data-testid="health-status-text">
            连通状态：{game.healthInfo.statusText}
            {game.healthInfo.tick !== undefined ? ` · Tick #${game.healthInfo.tick}` : ''}
          </div>
        ) : null}
      </div>

      <div className="lane">
        <div className="lane-title">
          <span>美术调试与样板透视</span>
          <span className="meta-note">已载入贴图 {game.loadedArtCount}/{ART_SLOT_LIST.length}</span>
        </div>
        <div className="row">
          <button
            type="button"
            className="btn primary"
            data-ui-id="W7.A1"
            data-testid="btn-reload-art"
            onClick={game.reloadArt}
          >
            <UiTag id="W7.A1" />
            热重载 public/assets
          </button>
          <button
            type="button"
            className={`btn ${game.showThreatOverlay ? 'active' : ''}`}
            data-ui-id="W7.D1"
            data-testid="btn-toggle-threat-field"
            onClick={() => {
              const next = !game.showThreatOverlay;
              game.setShowThreatOverlay(next);
              if (next) void game.refreshIField();
            }}
          >
            <UiTag id="W7.D1" />
            L5 i_threat 威胁场
          </button>
          <button
            type="button"
            className="btn danger"
            data-ui-id="W7.D2"
            data-testid="btn-sample-shake"
            onClick={game.triggerSampleShake}
          >
            <UiTag id="W7.D2" />
            测试爆破震屏
          </button>
        </div>
        <div className="row">
          <button
            type="button"
            className={`btn ${game.devSampleUnitDetail?.id === SAMPLE_SOLDIER_DETAIL.id ? 'active' : ''}`}
            data-ui-id="W7.D3"
            data-testid="btn-sample-soldier"
            onClick={() => {
              game.setDevSampleUnitDetail(
                game.devSampleUnitDetail?.id === SAMPLE_SOLDIER_DETAIL.id ? null : SAMPLE_SOLDIER_DETAIL,
              );
              game.openWindow('W3');
            }}
          >
            <UiTag id="W7.D3" />
            预览单兵4部位卡
          </button>
          <button
            type="button"
            className={`btn ${game.devSampleUnitDetail?.id === SAMPLE_VEHICLE_DETAIL.id ? 'active' : ''}`}
            data-ui-id="W7.D4"
            data-testid="btn-sample-tank"
            onClick={() => {
              game.setDevSampleUnitDetail(
                game.devSampleUnitDetail?.id === SAMPLE_VEHICLE_DETAIL.id ? null : SAMPLE_VEHICLE_DETAIL,
              );
              game.openWindow('W3');
            }}
          >
            <UiTag id="W7.D4" />
            预览坦克8部件卡
          </button>
          <button
            type="button"
            className="btn"
            data-ui-id="W7.D6"
            data-testid="btn-open-example-win"
            onClick={() => game.toggleWindow('W8')}
          >
            <UiTag id="W7.D6" />
            开/关 W8 示例模板
          </button>
        </div>
        <div className="art-slot-list">
          {slotStatuses.map(({ spec, status }) => (
            <div key={spec.key} className="art-slot-item">
              <code>{spec.key}</code>
              <span className="meta-note">
                {spec.layer} · {spec.recommendedPx} ·{' '}
                {status === 'loaded' ? '已命中PNG' : status === 'loading' ? '加载中' : '矢量兜底'}
              </span>
            </div>
          ))}
        </div>
      </div>

      <div className="row">
        <button
          type="button"
          className="btn"
          data-ui-id="W7.W1"
          onClick={game.resetWindows}
        >
          <UiTag id="W7.W1" />
          恢复全部窗口默认位置
        </button>
      </div>
    </div>
  );
};
