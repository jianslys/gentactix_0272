import { describe, expect, it } from 'vitest';
import { computeCommandDelay, expandDirective, remoteDelayText } from '../engine/directives';
import { LocalMockSandboxAdapter } from '../engine/localMockSandbox';
import { progressiveSnapshot } from '../engine/playback';
import { HQ_UNIT_ID, PRESET_CHARGE, SQUAD1_ID, SQUAD3_ID } from '../engine/scenario';
import { StepCache, StepCacheRangeError } from '../engine/stepCache';
import { fnv1a64, mixTimeUnitHash, obfuscateIssue, queryTimeLut, splitmix64 } from '../engine/timeMachine';
import { MAX_STEP_CACHE_HORIZON_TICKS, TICK_MS } from '../engine/types';

describe('Phase 1 · 双哈希时间混淆 & 60 天 StepCache & 16s 防剧透沙盒', () => {
  it('SplitMix64 + FNV-1a + 5×16 LUT 确定性与 64 位无符号一致性', () => {
    expect(splitmix64(0n)).toBe(0xe220a8397b1dcdafn);
    expect(fnv1a64('1 班')).toBe(fnv1a64('1 班'));
    const h1 = mixTimeUnitHash(16, '1 班');
    const h2 = mixTimeUnitHash(16, '1 班');
    expect(h1).toBe(h2);
    const e = queryTimeLut('movement', h1);
    expect(e.turnOffset).toBeGreaterThanOrEqual(0);
    expect(e.exactMs).toBeGreaterThanOrEqual(1);
    expect(e.exactMs).toBeLessThanOrEqual(16000);

    const obf = obfuscateIssue(0, '1 班', 'movement');
    expect(obf.anchorTick).toBe(1 + obf.entry.turnOffset);
  });

  it('StepCache 60 天边界：#324000 合法，#324001 拒绝，且稀疏桶不占空白回合', () => {
    const cache = new StepCache();
    expect(MAX_STEP_CACHE_HORIZON_TICKS).toBe(324_000);

    cache.schedule(0, {
      eventId: 'ev-60d',
      team: 'red',
      sourceTick: 0,
      targetTick: 324_000,
      secInTick: 1,
      exactMs: 500,
      kind: 'detonation',
      payload: { type: 'detonation', radiusM: 12, label: '60天边界爆破' },
    });
    expect(cache.bucketCount).toBe(1);
    expect(cache.lookahead(324_000, 'red')).toHaveLength(1);

    expect(() =>
      cache.schedule(0, {
        eventId: 'ev-overflow',
        team: 'red',
        sourceTick: 0,
        targetTick: 324_001,
        secInTick: 1,
        exactMs: 500,
        kind: 'detonation',
        payload: { type: 'detonation', radiusM: 12, label: '越界' },
      }),
    ).toThrow(StepCacheRangeError);

    expect(() =>
      cache.schedule(5, {
        eventId: 'ev-past',
        team: 'red',
        sourceTick: 5,
        targetTick: 5,
        secInTick: 1,
        exactMs: 500,
        kind: 'detonation',
        payload: { type: 'detonation', radiusM: 12, label: '当前回合' },
      }),
    ).toThrow(StepCacheRangeError);
  });

  it('#3 前向窥视 #4 己方定时炸药且严格过滤敌方炸药（迷雾隔离）', async () => {
    const adapter = new LocalMockSandboxAdapter();
    await adapter.fastForwardTo(3, 'red');
    expect(adapter.getCurrentTick()).toBe(3);

    const redPreview = await adapter.lookahead(4, 'red');
    const redDetonations = redPreview.filter((e) => e.kind === 'detonation');
    expect(redDetonations.map((e) => e.eventId)).toEqual([PRESET_CHARGE.eventId]);

    const bluePreview = await adapter.lookahead(4, 'blue');
    expect(bluePreview.filter((e) => e.kind === 'detonation').map((e) => e.eventId)).toEqual(['preset-charge-blue-4']);
  });

  it('远程指挥偏移计算：直属 1 班零延迟，远程 3 班自动给出正向延迟拍数与文案', async () => {
    const adapter = new LocalMockSandboxAdapter();
    const init = await adapter.loadInitial('red');
    const hq = init.snapshot.units.find((u) => u.id === HQ_UNIT_ID)!;
    const sq1 = init.snapshot.units.find((u) => u.id === SQUAD1_ID)!;
    const sq3 = init.snapshot.units.find((u) => u.id === SQUAD3_ID)!;

    const d1 = computeCommandDelay(hq, sq1, init.directCommandRadiusM);
    expect(d1.remote).toBe(false);
    expect(d1.delayTicks).toBe(0);

    const d3 = computeCommandDelay(hq, sq3, init.directCommandRadiusM);
    expect(d3.remote).toBe(true);
    expect(d3.delayTicks).toBe(2);
    expect(remoteDelayText(d3)).toContain('+2 拍');
    expect(remoteDelayText(d3)).toContain('+32s');
  });

  it('16s 走表防剧透：t=0s 不提前暴露弹坑或终点坐标，到达 exactMs/16s 渐进兑现', async () => {
    const adapter = new LocalMockSandboxAdapter();
    // 给 1 班签发移动指令并推进至生效拍
    const atoms = expandDirective({
      id: 'dir-move-1',
      seq: 1,
      mode: 'task',
      taskKind: 'move_unit',
      unitId: SQUAD1_ID,
      target: { type: 'point', pos: { x: 260, y: 240 } },
      startSec: 0,
      endSec: 12,
      delayOffsetSec: 0,
      delayMultiplier: 1,
      stance: 'normal',
      discipline: 'return_fire',
      work: 'foxhole_line',
      origin: 'new',
    });
    const receipt = await adapter.issueQueue({
      directives: [{ directiveId: 'dir-move-1', unitId: SQUAD1_ID, atoms }],
    });
    const firstTick = receipt.receipts[0]!.firstTick;
    const movePlaybacks = await adapter.fastForwardTo(firstTick, 'red');
    const movePb = movePlaybacks[movePlaybacks.length - 1]!;

    const atStart = progressiveSnapshot(movePb, 0);
    const atMid = progressiveSnapshot(movePb, Math.min(TICK_MS - 500, receipt.receipts[0]!.firstExactMs + 6000));
    const atEnd = progressiveSnapshot(movePb, TICK_MS);

    const sqStart = atStart.units.find((u) => u.id === SQUAD1_ID)!;
    const sqMid = atMid.units.find((u) => u.id === SQUAD1_ID)!;
    const sqEnd = atEnd.units.find((u) => u.id === SQUAD1_ID)!;
    expect(sqStart.pos.x).toBeCloseTo(movePb.baseline.units.find((u) => u.id === SQUAD1_ID)!.pos.x, 4);
    expect(sqEnd.pos.x).toBeGreaterThan(sqStart.pos.x);
    expect(sqMid.pos.x).toBeGreaterThan(sqStart.pos.x);
    expect(sqMid.pos.x).toBeLessThanOrEqual(sqEnd.pos.x);

    // 继续推演至 #4 定时炸药起爆回合
    const pbs = await adapter.fastForwardTo(4, 'red');
    const pb4 = pbs[pbs.length - 1]!;
    const beforeDet = progressiveSnapshot(pb4, 0);
    const justBeforeDet = progressiveSnapshot(pb4, PRESET_CHARGE.exactMs - 1);
    const atDet = progressiveSnapshot(pb4, PRESET_CHARGE.exactMs);
    const end4 = progressiveSnapshot(pb4, TICK_MS);

    const hasRedCrater = (snap: ReturnType<typeof progressiveSnapshot>) =>
      snap.destruction.some(
        (d) => d.kind === 'crater' && d.pos.x === PRESET_CHARGE.pos.x && d.pos.y === PRESET_CHARGE.pos.y,
      );
    expect(hasRedCrater(beforeDet)).toBe(false);
    expect(hasRedCrater(justBeforeDet)).toBe(false);
    expect(hasRedCrater(atDet)).toBe(true);
    expect(end4).toEqual({ ...pb4.final, elapsedMs: TICK_MS });
  });
});
