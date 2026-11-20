import { Component, type ErrorInfo, type ReactNode } from 'react';

interface ErrorBoundaryProps {
  children: ReactNode;
  fallbackTitle?: string;
  onReset?: () => void;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  public state: ErrorBoundaryState = {
    hasError: false,
    error: null,
  };

  public static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  public componentDidCatch(error: Error, errorInfo: ErrorInfo): void {
    console.error('[GENTACTIX ErrorBoundary Caught]', error, errorInfo);
  }

  private handleReset = (): void => {
    this.setState({ hasError: false, error: null });
    this.props.onReset?.();
  };

  private handleReload = (): void => {
    window.location.reload();
  };

  public render(): ReactNode {
    if (this.state.hasError) {
      return (
        <div
          data-testid="error-boundary-fallback"
          style={{
            width: '100vw',
            height: '100vh',
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            background: '#0d1419',
            color: '#e2ddd1',
            fontFamily: '"Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif',
            padding: '24px',
            boxSizing: 'border-box',
          }}
        >
          <div
            style={{
              maxWidth: '620px',
              width: '100%',
              background: '#111a21',
              border: '1px solid rgba(201, 74, 61, 0.65)',
              borderRadius: '4px',
              padding: '24px 28px',
              boxShadow: '0 16px 48px rgba(0, 0, 0, 0.85)',
              display: 'flex',
              flexDirection: 'column',
              gap: '14px',
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              <span
                style={{
                  background: '#c94a3d',
                  color: '#ffffff',
                  fontSize: '10px',
                  fontWeight: 800,
                  padding: '2px 6px',
                  borderRadius: '2px',
                  fontFamily: 'monospace',
                }}
              >
                CRITICAL WARNING
              </span>
              <span style={{ fontSize: '12px', color: '#8ba0ae', letterSpacing: '0.1em', textTransform: 'uppercase', fontFamily: 'monospace' }}>
                GENTACTIX / EXCEPTION SHIELD
              </span>
            </div>

            <div style={{ fontSize: '18px', fontWeight: 800, color: '#ffffff' }}>
              {this.props.fallbackTitle ?? '战场态势渲染异常中断 (UI RENDER EXCEPTION)'}
            </div>

            <p style={{ margin: 0, fontSize: '12px', color: '#8ba0ae', lineHeight: 1.6 }}>
              系统已成功拦截未处理异常，沙盘主进程未丢失。您可以重置本地图层与摄像机坐标，或刷新页面恢复最新态势。
            </p>

            {this.state.error && (
              <div
                style={{
                  background: 'rgba(5, 10, 14, 0.85)',
                  border: '1px solid rgba(82, 108, 124, 0.35)',
                  borderRadius: '3px',
                  padding: '10px 12px',
                  fontSize: '11px',
                  fontFamily: 'Consolas, monospace',
                  color: '#f87171',
                  wordBreak: 'break-all',
                  maxHeight: '140px',
                  overflowY: 'auto',
                }}
              >
                {this.state.error.name}: {this.state.error.message}
              </div>
            )}

            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '10px', marginTop: '6px' }}>
              <button
                type="button"
                className="btn"
                style={{
                  background: '#1a2732',
                  color: '#e2ddd1',
                  border: '1px solid #3b5060',
                  padding: '6px 14px',
                  fontSize: '12px',
                  cursor: 'pointer',
                  borderRadius: '3px',
                }}
                onClick={this.handleReset}
              >
                ↺ 尝试在位恢复
              </button>
              <button
                type="button"
                className="btn primary"
                style={{
                  background: '#1b3548',
                  color: '#ffffff',
                  border: '1px solid #5289ad',
                  padding: '6px 16px',
                  fontSize: '12px',
                  cursor: 'pointer',
                  borderRadius: '3px',
                  fontWeight: 600,
                }}
                onClick={this.handleReload}
              >
                强制刷新页面 (F5)
              </button>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
