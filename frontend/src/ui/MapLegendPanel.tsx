// GenTactix V2 frontend · 右下角图例与双地图/图层快捷控制面板（W10）

import React from 'react';
import type { MapViewMode } from '../render/canvasLayers';
import { UiTag } from './UiTag';

export interface MapLegendPanelProps {
  collapsed: boolean;
  onToggleCollapse: () => void;
  mapViewMode: MapViewMode;
  onToggleMapView: () => void;
  currentTick: number;
  showUnits: boolean;
  onToggleUnits: () => void;
  showGroundObjects: boolean;
  onToggleGroundObjects: () => void;
  showDestruction: boolean;
  onToggleDestruction: () => void;
  showCommandRings: boolean;
  onToggleCommandRings: () => void;
  showPlannedRoutes: boolean;
  onTogglePlannedRoutes: () => void;
  showWavesAndDetonations: boolean;
  onToggleWavesAndDetonations: () => void;
  showThreatOverlay: boolean;
  onToggleThreatOverlay: () => void;
  showAo: boolean;
  onToggleAo: () => void;
  l7Enabled: boolean;
  onToggleL7: () => void;
  showFreehand: boolean;
  onToggleFreehand: () => void;
  showUiIds: boolean;
  onToggleUiIds: () => void;
  l9Enabled: boolean;
  onToggleL9: () => void;
  onOpenClipboard: () => void;
  onOpenComposer: () => void;
  onStepNext: () => void;
}

export const MapLegendPanel: React.FC<MapLegendPanelProps> = ({
  collapsed,
  onToggleCollapse,
  mapViewMode,
  onToggleMapView,
  currentTick,
  showUnits,
  onToggleUnits,
  showGroundObjects,
  onToggleGroundObjects,
  showDestruction,
  onToggleDestruction,
  showCommandRings,
  onToggleCommandRings,
  showPlannedRoutes,
  onTogglePlannedRoutes,
  showWavesAndDetonations,
  onToggleWavesAndDetonations,
  showThreatOverlay,
  onToggleThreatOverlay,
  showAo,
  onToggleAo,
  l7Enabled,
  onToggleL7,
  showFreehand,
  onToggleFreehand,
  showUiIds,
  onToggleUiIds,
  l9Enabled,
  onToggleL9,
  onOpenClipboard,
  onOpenComposer,
  onStepNext,
}) => {
  return (
    <section className="basic-legend-panel" data-testid="legend-view-panel" data-ui-id="W10">
      <div className="basic-ui-header" data-ui-id="W10.HDR">
        <div className="basic-ui-title">
          <UiTag id="W10" kind="win" source="src/ui/MapLegendPanel.tsx" />
          <span>图例与地图视图</span>
        </div>
        <button
          type="button"
          className="basic-ui-btn"
          data-testid="legend-toggle"
          data-ui-id="W10.TOGGLE"
          onClick={onToggleCollapse}
        >
          <UiTag id="W10.TOGGLE" />
          {collapsed ? '展开' : '收起'}
        </button>
      </div>

      {!collapsed && (
        <div className="basic-legend-body">
          <div className="basic-legend-actions" data-ui-id="W10.MAP">
            <button
              type="button"
              className="basic-ui-btn basic-ui-btn-primary"
              style={{ width: '100%' }}
              data-testid="legend-switch-map"
              data-ui-id="W10.MAP_SWITCH"
              onClick={onToggleMapView}
            >
              <UiTag id="W10.MAP_SWITCH" />
              {mapViewMode === 'field' ? '切至副地图：军用印刷参谋图 [M]' : '切回主地图：近距现场视野 [M]'}
            </button>
          </div>

          <div className="basic-legend-section" data-ui-id="W10.VIEWS">
            <div className="basic-legend-subhdr">
              <UiTag id="W10.VIEWS" />
              <strong>战术图层开关</strong>
            </div>
            <div className="basic-view-grid">
              <label data-ui-id="V1">
                <UiTag id="V1" />
                <input type="checkbox" checked={showUnits} onChange={onToggleUnits} />
                <span>兵牌实体</span>
              </label>
              <label data-ui-id="V2">
                <UiTag id="V2" />
                <input type="checkbox" checked={showGroundObjects} onChange={onToggleGroundObjects} />
                <span>地面实例</span>
              </label>
              <label data-ui-id="V3">
                <UiTag id="V3" />
                <input type="checkbox" checked={showDestruction} onChange={onToggleDestruction} />
                <span>弹坑工事</span>
              </label>
              <label data-ui-id="V4">
                <UiTag id="V4" />
                <input type="checkbox" checked={showCommandRings} onChange={onToggleCommandRings} />
                <span>指挥通联圈</span>
              </label>
              <label data-ui-id="V5">
                <UiTag id="V5" />
                <input type="checkbox" checked={showPlannedRoutes} onChange={onTogglePlannedRoutes} />
                <span>计划航路</span>
              </label>
              <label data-ui-id="V6">
                <UiTag id="V6" />
                <input
                  type="checkbox"
                  checked={showWavesAndDetonations}
                  onChange={onToggleWavesAndDetonations}
                />
                <span>波次预警</span>
              </label>
              <label data-ui-id="V7">
                <UiTag id="V7" />
                <input type="checkbox" checked={showThreatOverlay} onChange={onToggleThreatOverlay} />
                <span>I场威胁图</span>
              </label>
              <label data-ui-id="V8">
                <UiTag id="V8" />
                <input type="checkbox" checked={showAo} onChange={onToggleAo} />
                <span>地形阴影AO</span>
              </label>
              <label data-ui-id="V9">
                <UiTag id="V9" />
                <input type="checkbox" checked={l7Enabled} onChange={onToggleL7} />
                <span>天气云雾</span>
              </label>
              <label data-ui-id="V10">
                <UiTag id="V10" />
                <input type="checkbox" checked={showFreehand} onChange={onToggleFreehand} />
                <span>手绘标绘</span>
              </label>
              <label data-ui-id="V11">
                <UiTag id="V11" />
                <input type="checkbox" checked={showUiIds} onChange={onToggleUiIds} />
                <span>显示UI ID</span>
              </label>
              <label data-ui-id="V14">
                <UiTag id="V14" />
                <input type="checkbox" checked={l9Enabled} onChange={onToggleL9} />
                <span>暗角特效</span>
              </label>
            </div>
          </div>

          <div className="basic-legend-section" data-ui-id="W10.LEGEND">
            <div className="basic-legend-subhdr">
              <UiTag id="W10.LEGEND" />
              <strong>地图图例（点图交互）</strong>
            </div>
            <div className="basic-legend-list">
              <div data-ui-id="LGD.1">
                <UiTag id="LGD.1" />
                <span className="lgd-swatch lgd-red" />
                <span className="lgd-swatch lgd-blue" />
                <span>己/敌兵牌（点开查编制与士兵物品）</span>
              </div>
              <div data-ui-id="LGD.2">
                <UiTag id="LGD.2" />
                <span className="lgd-swatch lgd-item" />
                <span className="lgd-swatch lgd-wpn" />
                <span>物资箱 / 散落武器（旁显基本信息）</span>
              </div>
              <div data-ui-id="LGD.3">
                <UiTag id="LGD.3" />
                <span className="lgd-symbol">×</span>
                <span>阵亡士兵遗体</span>
              </div>
              <div data-ui-id="LGD.4">
                <UiTag id="LGD.4" />
                <span className="lgd-swatch lgd-charge" />
                <span className="lgd-swatch lgd-mine" />
                <span>定时炸药 / 地雷</span>
              </div>
              <div data-ui-id="LGD.5">
                <UiTag id="LGD.5" />
                <span className="lgd-dash" />
                <span>直接指挥圈 / 预定航路</span>
              </div>
            </div>
          </div>

          <div className="basic-legend-actions" data-ui-id="W10.ACT">
            <button
              type="button"
              className="basic-ui-btn"
              data-testid="legend-open-w5"
              data-ui-id="V12"
              onClick={onOpenClipboard}
            >
              <UiTag id="V12" />
              手写板
            </button>
            <button
              type="button"
              className="basic-ui-btn"
              data-testid="legend-open-w12"
              data-ui-id="V15"
              onClick={onOpenComposer}
            >
              <UiTag id="V15" />
              写纸条
            </button>
            <button
              type="button"
              className="basic-ui-btn"
              data-testid="legend-step-next"
              data-ui-id="V13"
              onClick={onStepNext}
            >
              <UiTag id="V13" />
              推进+16s (#{currentTick})
            </button>
          </div>
        </div>
      )}
    </section>
  );
};
