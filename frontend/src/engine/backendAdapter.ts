// GenTactix V2 frontend · 唯一 BackendAdapter 接口层
// 前端状态机、手写板队列与 9 层渲染器严禁散落 fetch；一切引擎交互只经此接口。
// 第一阶段挂载 LocalMockSandboxAdapter；第二阶段切换 HttpEngineAdapter，UI 代码无需重写。

import type {
  CommandDelayInfo,
  InitialWorld,
  IssueQueueReceipt,
  IssueQueueRequest,
  IssueWaveRequest,
  ScheduledStepEvent,
  Team,
  TickPlayback,
  WaveDirective,
  WorldSnapshot,
} from './types';

export type BackendKind = 'local-mock' | 'http';

export interface BackendAdapter {
  readonly kind: BackendKind;
  /** 载入初始世界（实体、预置波次纸条、预渲染地形描述、指挥半径）。 */
  loadInitial(observer: Team): Promise<InitialWorld>;
  /** 当前已结算完毕的回合号。 */
  getCurrentTick(): number;
  /** 回合末权威快照（经阵营迷雾裁剪）。 */
  getSnapshot(observer: Team): Promise<WorldSnapshot>;
  /** 直接/远程指挥判定（纸条卡片实时明牌偏移提示）。 */
  previewCommandDelay(unitId: number): CommandDelayInfo | undefined;
  /**
   * 【签发任务队列】：提交已展开的原子指令包。
   * 时间混淆 + 远程偏移只在此处由后端（或第一阶段本地沙盒）唯一执行一次。
   */
  issueQueue(request: IssueQueueRequest): Promise<IssueQueueReceipt>;
  /** 【签发波次】：按直接/远程通联计算 G 场真实生效回合并写入步长缓存。 */
  issueWave(request: IssueWaveRequest): Promise<WaveDirective>;
  /** 推进一个 16s 步长：回合初锁定 [0..16000ms] 剧本并结算回合末权威态。 */
  advanceTick(observer: Team): Promise<TickPlayback>;
  /** 快进至目标回合（逐拍结算，空白回合稀疏跳跃）。 */
  fastForwardTo(targetTick: number, observer: Team): Promise<TickPlayback[]>;
  /** 前向窥视 StepCache[targetTick]：仅返回己方事件。 */
  lookahead(targetTick: number, observer: Team): Promise<ScheduledStepEvent[]>;
  /** 己方全部未来预约事件（迷雾过滤）。 */
  listScheduled(observer: Team): Promise<ScheduledStepEvent[]>;
}
