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
  bgApp: '#1b2428',
  bgPanel: '#242e32',
  bgPanelMuted: '#2c373b',
  bgFloatingTitle: '#1f292d',
  textPrimary: '#d6d9d4',
  textMuted: '#8f9b9b',
  borderSoft: '#3d484c',
  borderStrong: '#526064',
  redTeam: '#a94f43',
  blueTeam: '#436779',
  neutralTeam: '#777a67',
  accentAmber: '#c5aa70',
  accentGreen: '#91a58c',
  dangerRed: '#a94f43',
  terrainPaper: '#ddd7bd',
  contourStroke: 'rgba(157, 153, 120, 0.38)',
  riverFill: '#b4c4b9',
  roadBase: '#aa7057',
  fontUi: '"Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif',
  fontMono: '"Cascadia Code", "Consolas", monospace',
  radiusPanel: '2px',
  shadowWindow: '0 8px 30px rgba(16, 26, 30, 0.38)',
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
