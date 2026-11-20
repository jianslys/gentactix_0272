// GenTactix V2 frontend · 第二阶段真实后端适配器插座（对齐 FUNCTION_AND_INTERFACE_MANUAL.md）
// 注意：连真后端时，前端直接把展开后的原子指令发给 POST /api/command，由后端唯一计算时间混淆与远程偏移。

import type { BackendAdapter } from './backendAdapter';
import {
  type CommandDelayInfo,
  type InitialWorld,
  type IssueQueueReceipt,
  type IssueQueueRequest,
  type IssueWaveRequest,
  type ScheduledStepEvent,
  type Team,
  type TickPlayback,
  type WaveDirective,
  type WorldSnapshot,
} from './types';

export interface HttpEngineAdapterOptions {
  baseUrl: string;
}

export class HttpEngineAdapter implements BackendAdapter {
  readonly kind = 'http' as const;
  private readonly baseUrl: string;
  private cachedTick = 0;

  constructor(options: HttpEngineAdapterOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, '');
  }

  private async requestJson<T>(path: string, init?: RequestInit): Promise<T> {
    const res = await fetch(`${this.baseUrl}${path}`, {
      headers: { 'Content-Type': 'application/json' },
      ...init,
    });
    if (!res.ok) {
      throw new Error(`HTTP ${res.status} ${path}: ${await res.text()}`);
    }
    return (await res.json()) as T;
  }

  getCurrentTick(): number {
    return this.cachedTick;
  }

  async loadInitial(observer: Team): Promise<InitialWorld> {
    const snapshot = await this.getSnapshot(observer);
    return {
      snapshot,
      waves: [],
      hqUnitId: snapshot.units[0]?.id ?? 1,
      directCommandRadiusM: 140,
      radioRadiusM: 600,
      terrain: { widthM: 512, heightM: 512, hills: [], patches: [], river: [], roads: [] },
    };
  }

  async getSnapshot(observer: Team): Promise<WorldSnapshot> {
    const raw = await this.requestJson<{ tick?: number; units?: WorldSnapshot['units'] }>(`/api/state?team=${observer}`);
    this.cachedTick = raw.tick ?? this.cachedTick;
    return {
      tick: this.cachedTick,
      elapsedMs: 0,
      observer,
      units: raw.units ?? [],
      groundObjects: [],
      destruction: [],
      smoke: [],
      night: false,
    };
  }

  previewCommandDelay(_unitId: number): CommandDelayInfo | undefined {
    return undefined;
  }

  async issueQueue(request: IssueQueueRequest): Promise<IssueQueueReceipt> {
    const receipts: IssueQueueReceipt['receipts'] = [];
    for (const dir of request.directives) {
      for (const atom of dir.atoms) {
        await this.requestJson('/api/command', {
          method: 'POST',
          body: JSON.stringify(atom.command),
        });
      }
      receipts.push({
        directiveId: dir.directiveId,
        unitId: dir.unitId,
        category: 'command',
        lutSlot: 0,
        anchorTick: this.cachedTick + 1,
        anchorExactMs: 1000,
        remote: false,
        delayTicks: 0,
        firstTick: this.cachedTick + 1,
        firstExactMs: 1000,
        endTick: this.cachedTick + 1,
        endExactMs: 16000,
        eventIds: [],
      });
    }
    return { issueTick: this.cachedTick, receipts };
  }

  async issueWave(request: IssueWaveRequest): Promise<WaveDirective> {
    throw new Error(`后端波次接口待同步：${request.title}`);
  }

  async advanceTick(observer: Team): Promise<TickPlayback> {
    const stepRes = await this.requestJson<{ tick?: number }>('/api/clock/step', {
      method: 'POST',
      body: JSON.stringify({ ticks: 1, observer_team: observer }),
    });
    this.cachedTick = stepRes.tick ?? this.cachedTick + 1;
    const snap = await this.getSnapshot(observer);
    return {
      tick: this.cachedTick,
      observer,
      baseline: snap,
      keyframes: {},
      changes: [],
      log: [],
      shakeAtMs: [],
      completedDirectiveIds: [],
      effectiveWaveIds: [],
      final: snap,
    };
  }

  async fastForwardTo(targetTick: number, observer: Team): Promise<TickPlayback[]> {
    const out: TickPlayback[] = [];
    while (this.cachedTick < targetTick) {
      out.push(await this.advanceTick(observer));
    }
    return out;
  }

  async lookahead(_targetTick: number, _observer: Team): Promise<ScheduledStepEvent[]> {
    return [];
  }

  async listScheduled(_observer: Team): Promise<ScheduledStepEvent[]> {
    return [];
  }
}
