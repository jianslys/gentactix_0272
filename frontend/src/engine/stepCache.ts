// GenTactix V2 frontend · 60 天稀疏步长缓存（324,000 步长）
// 以目标回合号为 Key 的稀疏桶：空白回合零占用；前向窥视严格按阵营迷雾过滤。

import { MAX_STEP_CACHE_HORIZON_TICKS, type ScheduledStepEvent, type Team } from './types';

export class StepCacheRangeError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'StepCacheRangeError';
  }
}

const byExactMs = (a: ScheduledStepEvent, b: ScheduledStepEvent): number =>
  a.exactMs - b.exactMs || (a.eventId < b.eventId ? -1 : a.eventId > b.eventId ? 1 : 0);

export class StepCache {
  private readonly buckets = new Map<number, ScheduledStepEvent[]>();
  private readonly index = new Map<string, number>();

  /** 可预约窗口：currentTick + 1 ..= currentTick + 324_000。 */
  static isWithinHorizon(currentTick: number, targetTick: number): boolean {
    return (
      Number.isInteger(targetTick) &&
      targetTick > currentTick &&
      targetTick <= currentTick + MAX_STEP_CACHE_HORIZON_TICKS
    );
  }

  schedule(currentTick: number, event: ScheduledStepEvent): void {
    if (!StepCache.isWithinHorizon(currentTick, event.targetTick)) {
      throw new StepCacheRangeError(
        `StepCache 拒绝越界预约：#${event.targetTick} 不在 #${currentTick + 1}..=#${currentTick + MAX_STEP_CACHE_HORIZON_TICKS}（60 天）窗口内`,
      );
    }
    if (event.exactMs < 1 || event.exactMs > 16000) {
      throw new StepCacheRangeError(`exactMs ${event.exactMs} 超出 1..=16000`);
    }
    if (this.index.has(event.eventId)) {
      throw new StepCacheRangeError(`eventId 重复：${event.eventId}`);
    }
    const bucket = this.buckets.get(event.targetTick);
    if (bucket) {
      bucket.push(event);
      bucket.sort(byExactMs);
    } else {
      this.buckets.set(event.targetTick, [event]);
    }
    this.index.set(event.eventId, event.targetTick);
  }

  /** 前向窥视：仅返回 observerTeam 己方事件，绝不泄露敌方未爆物或计划。 */
  lookahead(targetTick: number, observerTeam: Team): ScheduledStepEvent[] {
    const bucket = this.buckets.get(targetTick);
    if (!bucket) return [];
    return bucket.filter((ev) => ev.team === observerTeam).map((ev) => ({ ...ev }));
  }

  /** 到期兑现：一次性弹出目标回合桶内全部事件（按 exactMs 排序）。 */
  drainDue(tick: number): ScheduledStepEvent[] {
    const bucket = this.buckets.get(tick);
    if (!bucket) return [];
    this.buckets.delete(tick);
    for (const ev of bucket) this.index.delete(ev.eventId);
    return bucket;
  }

  /** 中途注销：按 eventId 从目标回合桶中定点移除。 */
  cancel(eventId: string): boolean {
    const tick = this.index.get(eventId);
    if (tick === undefined) return false;
    const bucket = this.buckets.get(tick);
    this.index.delete(eventId);
    if (!bucket) return false;
    const next = bucket.filter((ev) => ev.eventId !== eventId);
    if (next.length === 0) this.buckets.delete(tick);
    else this.buckets.set(tick, next);
    return true;
  }

  has(eventId: string): boolean {
    return this.index.has(eventId);
  }

  /** 已占用（非空）桶数量——稀疏存储验证用。 */
  get bucketCount(): number {
    return this.buckets.size;
  }

  get eventCount(): number {
    return this.index.size;
  }

  /** 大于 afterTick 的最近非空桶回合号（快进跳跃用）。 */
  nextScheduledTick(afterTick: number): number | undefined {
    let best: number | undefined;
    for (const t of this.buckets.keys()) {
      if (t > afterTick && (best === undefined || t < best)) best = t;
    }
    return best;
  }

  /** 列出 observerTeam 己方全部未来事件（迷雾过滤）。 */
  listForTeam(observerTeam: Team): ScheduledStepEvent[] {
    const out: ScheduledStepEvent[] = [];
    for (const bucket of this.buckets.values()) {
      for (const ev of bucket) if (ev.team === observerTeam) out.push({ ...ev });
    }
    return out.sort((a, b) => a.targetTick - b.targetTick || byExactMs(a, b));
  }
}
