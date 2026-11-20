// GenTactix V2 frontend · 全局视觉 Theme Token（CSS 与 Canvas 唯一色板来源）
// 美术同学改配色时，只需改这一个文件里的 Hex 值，CSS 面板和 Canvas 战场会自动同步。

export interface ThemeTokens {
  bgApp: string;
  bgPanel: string;
  bgPanelMuted: string;
  bgFloatingTitle: string;
  textPrimary: string;
  textMuted: string;
  borderSoft: string;
  borderStrong: string;
  redTeam: string;
  blueTeam: string;
  neutralTeam: string;
  accentAmber: string;
  accentGreen: string;
  dangerRed: string;
  terrainPaper: string;
  contourStroke: string;
  riverFill: string;
  roadBase: string;
  fontUi: string;
  fontMono: string;
  radiusPanel: string;
  shadowWindow: string;
}

export const THEME_TOKENS: ThemeTokens = {
  bgApp: '#0d1419',
  bgPanel: '#111a21',
  bgPanelMuted: '#16222b',
  bgFloatingTitle: '#192732',
  textPrimary: '#e2ddd1',
  textMuted: '#8ba0ae',
  borderSoft: 'rgba(130, 155, 172, 0.22)',
  borderStrong: 'rgba(130, 155, 172, 0.45)',
  redTeam: '#b84d43',
  blueTeam: '#5b8296',
  neutralTeam: '#707d88',
  accentAmber: '#c89f55',
  accentGreen: '#4e9a68',
  dangerRed: '#c94a3d',
  terrainPaper: '#e4dec8',
  contourStroke: 'rgba(125, 110, 85, 0.32)',
  riverFill: 'rgba(110, 142, 160, 0.65)',
  roadBase: '#b47b62',
  fontUi: '"Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif',
  fontMono: '"Consolas", "JetBrains Mono", monospace',
  radiusPanel: '4px',
  shadowWindow: '0 12px 32px rgba(0, 0, 0, 0.65)',
};

/** 将 Theme Token 注入 `:root` CSS 变量，保证 HUD 与 Canvas 同色系。 */
export function applyThemeTokensToRoot(tokens: ThemeTokens = THEME_TOKENS, doc?: Document): void {
  const targetDoc = doc ?? (typeof document !== 'undefined' ? document : undefined);
  if (!targetDoc?.documentElement) return;
  const s = targetDoc.documentElement.style;
  s.setProperty('--gtx-bg-app', tokens.bgApp);
  s.setProperty('--gtx-bg-panel', tokens.bgPanel);
  s.setProperty('--gtx-bg-panel-muted', tokens.bgPanelMuted);
  s.setProperty('--gtx-bg-floating-title', tokens.bgFloatingTitle);
  s.setProperty('--gtx-text-primary', tokens.textPrimary);
  s.setProperty('--gtx-text-muted', tokens.textMuted);
  s.setProperty('--gtx-border-soft', tokens.borderSoft);
  s.setProperty('--gtx-border-strong', tokens.borderStrong);
  s.setProperty('--gtx-red-team', tokens.redTeam);
  s.setProperty('--gtx-blue-team', tokens.blueTeam);
  s.setProperty('--gtx-neutral-team', tokens.neutralTeam);
  s.setProperty('--gtx-accent-amber', tokens.accentAmber);
  s.setProperty('--gtx-accent-green', tokens.accentGreen);
  s.setProperty('--gtx-danger-red', tokens.dangerRed);
  s.setProperty('--gtx-terrain-paper', tokens.terrainPaper);
  s.setProperty('--gtx-font-ui', tokens.fontUi);
  s.setProperty('--gtx-font-mono', tokens.fontMono);
  s.setProperty('--gtx-radius-panel', tokens.radiusPanel);
  s.setProperty('--gtx-shadow-window', tokens.shadowWindow);
}
