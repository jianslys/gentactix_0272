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
  bgApp: '#f1ede2',
  bgPanel: '#faf8f2',
  bgPanelMuted: '#f3efe4',
  bgFloatingTitle: '#e6dfd0',
  textPrimary: '#1c1917',
  textMuted: '#57534e',
  borderSoft: '#d6cfc0',
  borderStrong: '#a89f8d',
  redTeam: '#b9382c',
  blueTeam: '#2b5f9e',
  neutralTeam: '#6b7280',
  accentAmber: '#d97706',
  accentGreen: '#15803d',
  dangerRed: '#dc2626',
  terrainPaper: '#dfd9c5',
  contourStroke: 'rgba(120, 102, 74, 0.34)',
  riverFill: 'rgba(72, 128, 168, 0.56)',
  roadBase: '#968262',
  fontUi: '"Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif',
  fontMono: '"Consolas", "JetBrains Mono", monospace',
  radiusPanel: '6px',
  shadowWindow: '0 8px 24px rgba(24, 21, 18, 0.2)',
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
