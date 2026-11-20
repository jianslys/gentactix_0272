// GenTactix V2 frontend · 双哈希时间混淆器（SplitMix64 + FNV-1a + 5×16 品类表）
// 1:1 复刻后端 `time_machine.rs` 口径：全部 64 位运算使用 BigInt.asUintN(64, ...)。
// 注意：第二阶段接入 HttpEngineAdapter 后，时间混淆由后端唯一执行，前端不得二次延迟。

import type { TacticalCommandJson, TimeCategory, TimeLutEntry } from './types';

const U64 = (v: bigint): bigint => BigInt.asUintN(64, v);

export const SPLITMIX64_GAMMA = 0x9e3779b97f4a7c15n;
export const SPLITMIX64_MUL_1 = 0xbf58476d1ce4e5b9n;
export const SPLITMIX64_MUL_2 = 0x94d049bb133111ebn;
export const FNV1A_64_OFFSET = 0xcbf29ce484222325n;
export const FNV1A_64_PRIME = 0x100000001b3n;

/** SplitMix64 混合函数（加黄金增量后执行 30/27/31 移位乘法混合）。 */
export function splitmix64(input: bigint): bigint {
  let z = U64(input + SPLITMIX64_GAMMA);
  z = U64((z ^ (z >> 30n)) * SPLITMIX64_MUL_1);
  z = U64((z ^ (z >> 27n)) * SPLITMIX64_MUL_2);
  return U64(z ^ (z >> 31n));
}

/** 按 UTF-8 字节迭代的 FNV-1a 64 位哈希。 */
export function fnv1a64(text: string): bigint {
  const bytes = new TextEncoder().encode(text);
  let h = FNV1A_64_OFFSET;
  for (const b of bytes) {
    h = U64((h ^ BigInt(b)) * FNV1A_64_PRIME);
  }
  return h;
}

export function rotl64(x: bigint, r: number): bigint {
  const rr = BigInt(r & 63);
  const v = U64(x);
  if (rr === 0n) return v;
  return U64((v << rr) | (v >> (64n - rr)));
}

/** mixTimeUnitHash(absSec, unitName) = splitmix64(splitmix64(absSec) ^ rotl64(fnv1a64(unitName), 27)) */
export function mixTimeUnitHash(absSec: number | bigint, unitName: string): bigint {
  const abs = U64(typeof absSec === 'bigint' ? absSec : BigInt(Math.trunc(absSec)));
  return splitmix64(splitmix64(abs) ^ rotl64(fnv1a64(unitName), 27));
}

const e = (turnOffset: number, secInTick: number, exactMs: number): TimeLutEntry =>
  Object.freeze({ turnOffset, secInTick, exactMs });

export const LUT_BULLET: readonly TimeLutEntry[] = Object.freeze([
  e(0, 1, 320), e(0, 1, 580), e(0, 1, 840), e(0, 2, 1120), e(0, 2, 1450), e(0, 2, 1780), e(0, 2, 2010), e(0, 3, 2320),
  e(0, 3, 2650), e(0, 3, 2980), e(0, 3, 3210), e(0, 4, 3450), e(0, 4, 3680), e(0, 4, 3910), e(0, 4, 4150), e(0, 4, 4380),
]);

export const LUT_SHELL: readonly TimeLutEntry[] = Object.freeze([
  e(1, 4, 4200), e(1, 5, 5350), e(1, 6, 6480), e(1, 7, 7620), e(1, 8, 8740), e(1, 9, 9860), e(1, 10, 10950), e(1, 11, 11800),
  e(1, 12, 12650), e(1, 13, 13420), e(1, 14, 14190), e(1, 15, 14920), e(1, 16, 15680), e(1, 5, 5720), e(1, 8, 8910), e(1, 11, 11950),
]);

export const LUT_COMMAND: readonly TimeLutEntry[] = Object.freeze([
  e(0, 3, 3100), e(0, 4, 4250), e(0, 5, 5400), e(0, 6, 6600), e(0, 7, 7800), e(0, 8, 8950), e(0, 9, 9980), e(0, 10, 10850),
  e(0, 11, 11720), e(0, 12, 12550), e(1, 2, 2100), e(1, 3, 3300), e(1, 4, 4500), e(0, 4, 4680), e(0, 7, 7420), e(0, 9, 9650),
]);

export const LUT_EVENT: readonly TimeLutEntry[] = Object.freeze([
  e(0, 5, 5200), e(0, 6, 6400), e(0, 7, 7700), e(0, 8, 8900), e(0, 9, 9950), e(0, 10, 10880), e(0, 11, 11750), e(0, 12, 12410),
  e(0, 13, 13200), e(0, 14, 14100), e(0, 15, 15050), e(0, 16, 15900), e(1, 4, 4300), e(1, 6, 6700), e(1, 8, 8850), e(0, 12, 12890),
]);

export const LUT_MOVEMENT: readonly TimeLutEntry[] = Object.freeze([
  e(0, 1, 1100), e(0, 1, 1450), e(0, 2, 2200), e(0, 2, 2600), e(0, 3, 3150), e(0, 3, 3550), e(0, 4, 4100), e(0, 4, 4500),
  e(0, 5, 5150), e(0, 5, 5650), e(0, 6, 6100), e(0, 6, 6600), e(0, 2, 2450), e(0, 3, 3750), e(0, 4, 4850), e(0, 5, 5900),
]);

export const TIME_LUTS: Readonly<Record<TimeCategory, readonly TimeLutEntry[]>> = Object.freeze({
  bullet: LUT_BULLET,
  shell: LUT_SHELL,
  command: LUT_COMMAND,
  event: LUT_EVENT,
  movement: LUT_MOVEMENT,
});

/** 槽位 = Number(hashM & 0x0f)。 */
export function lutSlot(hashM: bigint): number {
  return Number(hashM & 0x0fn);
}

export function queryTimeLut(category: TimeCategory, hashM: bigint): TimeLutEntry {
  const table = TIME_LUTS[category];
  const entry = table[lutSlot(hashM)];
  if (!entry) throw new Error(`time LUT ${category} slot out of range`);
  return entry;
}

/** 指令 -> 时间品类（与手册 TacticalCommand 类别列一致）。 */
export function categoryForCommand(cmd: TacticalCommandJson): TimeCategory {
  switch (cmd.kind) {
    case 'move_unit':
      return 'movement';
    case 'attack_pos':
    case 'attack_unit':
      return 'bullet';
    case 'build_work':
      return 'event';
    case 'hold_unit':
    case 'set_fire_discipline':
    case 'pop_smoke':
      return 'command';
  }
}

export interface ObfuscatedAnchor {
  category: TimeCategory;
  hash: bigint;
  slot: number;
  entry: TimeLutEntry;
  /** 锚点回合 = issueTick + 1 + turnOffset（即 #k+1 或 #k+2）。 */
  anchorTick: number;
  anchorExactMs: number;
}

/** 对一次签发执行“唯一一次”双哈希时间混淆，得到锚点回合与毫秒。 */
export function obfuscateIssue(issueTick: number, unitName: string, category: TimeCategory, cmdSecInTick = 0): ObfuscatedAnchor {
  const absSec = issueTick * 16 + cmdSecInTick;
  const hash = mixTimeUnitHash(absSec, unitName);
  const entry = queryTimeLut(category, hash);
  return {
    category,
    hash,
    slot: lutSlot(hash),
    entry,
    anchorTick: issueTick + 1 + entry.turnOffset,
    anchorExactMs: entry.exactMs,
  };
}

/** 绝对毫秒 -> (targetTick, exactMs ∈ 1..=16000)。 */
export function absMsToSlot(absMs: number): { tick: number; exactMs: number; secInTick: number } {
  const v = Math.max(1, Math.round(absMs));
  let tick = Math.floor(v / 16000);
  let exactMs = v - tick * 16000;
  if (exactMs === 0) {
    tick -= 1;
    exactMs = 16000;
  }
  const secInTick = Math.min(16, Math.max(1, Math.ceil(exactMs / 1000)));
  return { tick, exactMs, secInTick };
}

export function slotToAbsMs(tick: number, exactMs: number): number {
  return tick * 16000 + exactMs;
}
