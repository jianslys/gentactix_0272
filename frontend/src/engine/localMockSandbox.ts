// GenTactix V2 frontend · 第一阶段内置模拟沙盒 + 16s 逐秒防剧透走表器

import type { BackendAdapter } from './backendAdapter';
import { computeCommandDelay, distance } from './directives';
import {
  DIRECT_COMMAND_RADIUS_M,
  HQ_UNIT_ID,
  PRESET_CHARGE,
  PRESET_ENEMY_CHARGE,
  RADIO_RADIUS_M,
  VISION_RADIUS_M,
  WORLD_HEIGHT_M,
  WORLD_WIDTH_M,
  presetGroundObjects,
  presetTerrain,
  presetUnits,
  presetWaves,
} from './scenario';
import { StepCache } from './stepCache';
import { absMsToSlot, categoryForCommand, obfuscateIssue, slotToAbsMs } from './timeMachine';
import {
  TICK_MS,
  TICK_SECONDS,
  type CommandDelayInfo,
  type DirectiveIssueReceipt,
  type DiscreteChange,
  type GroundObject,
  type InitialWorld,
  type IssueQueueReceipt,
  type IssueQueueRequest,
  type IssueWaveRequest,
  type PlaybackLogEntry,
  type ScheduledStepEvent,
  type SmokeCloud,
  type TacticalCommandJson,
  type Team,
  type TerrainDestruction,
  type TerrainModel,
  type TickPlayback,
  type UnitKeyframe,
  type UnitState,
  type Vec2,
  type WaveDirective,
  type WorldSnapshot,
} from './types';

const WALK_SPEED_MPS = 2.5;

function speedForStance(stance?: string): number {
  if (stance === 'fast') return 3.6;
  if (stance === 'stealth') return 1.6;
  return WALK_SPEED_MPS;
}

function clampPos(p: Vec2): Vec2 {
  return {
    x: Math.max(8, Math.min(WORLD_WIDTH_M - 8, p.x)),
    y: Math.max(8, Math.min(WORLD_HEIGHT_M - 8, p.y)),
  };
}

function cloneUnit(u: UnitState): UnitState {
  const activity =
    u.activity.type === 'move'
      ? { type: 'move' as const, path: u.activity.path.map((p) => ({ ...p })) }
      : u.activity.type === 'build'
        ? { type: 'build' as const, work: u.activity.work, points: u.activity.points.map((p) => ({ ...p })) }
        : u.activity.type === 'fire'
          ? { type: 'fire' as const, target: { ...u.activity.target }, targetUnitId: u.activity.targetUnitId }
          : u.activity.type === 'smoke'
            ? { type: 'smoke' as const, target: { ...u.activity.target } }
            : { ...u.activity };
  return { ...u, pos: { ...u.pos }, activity };
}

function cloneWave(w: WaveDirective): WaveDirective {
  return {
    ...w,
    geometry: {
      barrages: w.geometry.barrages.map((b) =>
        b.type === 'circle'
          ? { type: 'circle', center: { ...b.center }, radiusM: b.radiusM }
          : { type: 'polygon', points: b.points.map((p) => ({ ...p })) },
      ),
      assaultArrows: w.geometry.assaultArrows.map((arr) => arr.map((p) => ({ ...p }))),
      aos: w.geometry.aos.map((a) => ({ min: { ...a.min }, max: { ...a.max } })),
    },
  };
}

export class LocalMockSandboxAdapter implements BackendAdapter {
  readonly kind = 'local-mock' as const;

  private tick = 0;
  private units: UnitState[];
  private groundObjects: GroundObject[];
  private destruction: TerrainDestruction[] = [];
  private smoke: SmokeCloud[] = [];
  private waves: WaveDirective[];
  private terrain: TerrainModel;
  private stepCache = new StepCache();
  private waveSeq = 3;
  private eventSeq = 1;
  private night = false;

  constructor() {
    this.units = presetUnits();
    this.groundObjects = presetGroundObjects();
    this.waves = presetWaves();
    this.terrain = presetTerrain();

    // 预埋 #4 回合起爆的己方定时炸药 & 敌方定时炸药（测试迷雾过滤）
    this.stepCache.schedule(0, {
      eventId: PRESET_CHARGE.eventId,
      team: 'red',
      sourceTick: 0,
      targetTick: PRESET_CHARGE.targetTick,
      secInTick: Math.ceil(PRESET_CHARGE.exactMs / 1000),
      exactMs: PRESET_CHARGE.exactMs,
      kind: 'detonation',
      targetPos: { ...PRESET_CHARGE.pos },
      payload: {
        type: 'detonation',
        radiusM: PRESET_CHARGE.radiusM,
        label: '定时炸药起爆（#0 预埋）',
      },
    });

    this.stepCache.schedule(0, {
      eventId: PRESET_ENEMY_CHARGE.eventId,
      team: 'blue',
      sourceTick: 0,
      targetTick: PRESET_ENEMY_CHARGE.targetTick,
      secInTick: Math.ceil(PRESET_ENEMY_CHARGE.exactMs / 1000),
      exactMs: PRESET_ENEMY_CHARGE.exactMs,
      kind: 'detonation',
      targetPos: { ...PRESET_ENEMY_CHARGE.pos },
      payload: {
        type: 'detonation',
        radiusM: PRESET_ENEMY_CHARGE.radiusM,
        label: '敌方定时炸药（迷雾隔离）',
      },
    });

    // 预埋预置波次入 StepCache
    for (const w of this.waves) {
      this.stepCache.schedule(0, {
        eventId: `wave-ev-${w.id}`,
        team: 'red',
        sourceTick: 0,
        targetTick: w.effectiveTick,
        secInTick: Math.ceil(w.effectiveExactMs / 1000),
        exactMs: w.effectiveExactMs,
        kind: 'g_field_wave',
        payload: { type: 'g_field_wave', waveId: w.id, label: w.title },
      });
    }
  }

  getCurrentTick(): number {
    return this.tick;
  }

  private isVisibleTo(unit: UnitState, observer: Team): boolean {
    if (unit.team === observer || observer === 'neutral') return true;
    const friendlies = this.units.filter((u) => u.team === observer && u.alive);
    return friendlies.some((f) => distance(f.pos, unit.pos) <= VISION_RADIUS_M);
  }

  private snapshotFor(observer: Team, elapsedMs = 0): WorldSnapshot {
    return {
      tick: this.tick,
      elapsedMs,
      observer,
      units: this.units.filter((u) => this.isVisibleTo(u, observer)).map(cloneUnit),
      groundObjects: this.groundObjects.map((g) => ({ ...g, pos: { ...g.pos } })),
      destruction: this.destruction.map((d) => ({
        ...d,
        pos: { ...d.pos },
        points: d.points?.map((p) => ({ ...p })),
      })),
      smoke: this.smoke.map((s) => ({ ...s, pos: { ...s.pos } })),
      night: this.night,
    };
  }

  async loadInitial(observer: Team): Promise<InitialWorld> {
    return {
      snapshot: this.snapshotFor(observer, 0),
      waves: this.waves.map(cloneWave),
      hqUnitId: HQ_UNIT_ID,
      directCommandRadiusM: DIRECT_COMMAND_RADIUS_M,
      radioRadiusM: RADIO_RADIUS_M,
      terrain: this.terrain,
    };
  }

  async getSnapshot(observer: Team): Promise<WorldSnapshot> {
    return this.snapshotFor(observer, 0);
  }

  previewCommandDelay(unitId: number): CommandDelayInfo | undefined {
    const hq = this.units.find((u) => u.id === HQ_UNIT_ID);
    const unit = this.units.find((u) => u.id === unitId);
    if (!unit) return undefined;
    return computeCommandDelay(hq, unit, DIRECT_COMMAND_RADIUS_M);
  }

  async issueQueue(request: IssueQueueRequest): Promise<IssueQueueReceipt> {
    const issueTick = this.tick;
    const hq = this.units.find((u) => u.id === HQ_UNIT_ID);
    const receipts: DirectiveIssueReceipt[] = [];

    for (const dir of request.directives) {
      const unit = this.units.find((u) => u.id === dir.unitId);
      if (!unit) throw new Error(`未找到单位 #${dir.unitId}`);
      if (dir.atoms.length === 0) throw new Error(`纸条 ${dir.directiveId} 原子指令为空`);

      const delay = computeCommandDelay(hq, unit, DIRECT_COMMAND_RADIUS_M);
      const firstAtom = dir.atoms[0]!;
      const cat = categoryForCommand(firstAtom.command);
      // 唯一一次双哈希时间混淆
      const obf = obfuscateIssue(issueTick, unit.name, cat, 0);
      const anchorAbsMs = slotToAbsMs(obf.anchorTick + delay.delayTicks, obf.anchorExactMs);

      const eventIds: string[] = [];
      let firstTick = obf.anchorTick + delay.delayTicks;
      let firstExactMs = obf.anchorExactMs;
      let endTick = firstTick;
      let endExactMs = firstExactMs;

      dir.atoms.forEach((atom, idx) => {
        const slot = absMsToSlot(anchorAbsMs + Math.round(atom.offsetSec * 1000));
        if (idx === 0) {
          firstTick = slot.tick;
          firstExactMs = slot.exactMs;
        }
        if (idx === dir.atoms.length - 1) {
          endTick = slot.tick;
          endExactMs = slot.exactMs;
        }
        const evId = `cmd-${issueTick}-${this.eventSeq++}`;
        eventIds.push(evId);
        this.stepCache.schedule(issueTick, {
          eventId: evId,
          team: unit.team,
          sourceTick: issueTick,
          targetTick: slot.tick,
          secInTick: slot.secInTick,
          exactMs: slot.exactMs,
          kind: atom.isStop ? 'macro_stop' : 'task_command',
          subjectUnitId: unit.id,
          payload: atom.isStop
            ? {
                type: 'macro_stop',
                directiveId: dir.directiveId,
                command: atom.command,
                label: `${unit.name} · ${atom.label}`,
              }
            : {
                type: 'task_command',
                directiveId: dir.directiveId,
                command: atom.command,
                label: `${unit.name} · ${atom.label}`,
              },
        });
      });

      receipts.push({
        directiveId: dir.directiveId,
        unitId: unit.id,
        category: cat,
        lutSlot: obf.slot,
        anchorTick: obf.anchorTick,
        anchorExactMs: obf.anchorExactMs,
        remote: delay.remote,
        delayTicks: delay.delayTicks,
        firstTick,
        firstExactMs,
        endTick,
        endExactMs,
        eventIds,
      });
    }

    return { issueTick, receipts };
  }

  async issueWave(request: IssueWaveRequest): Promise<WaveDirective> {
    const hq = this.units.find((u) => u.id === HQ_UNIT_ID);
    const id = `wave-${this.waveSeq++}`;
    const obf = obfuscateIssue(this.tick, request.title || id, 'shell', 0);
    // 计算波次几何中心相对 HQ 的通联判定
    const focus: Vec2 =
      request.geometry.barrages[0]?.type === 'circle'
        ? request.geometry.barrages[0].center
        : request.geometry.barrages[0]?.type === 'polygon' && request.geometry.barrages[0].points[0]
          ? request.geometry.barrages[0].points[0]
          : request.geometry.assaultArrows[0]?.[0] ?? (hq ? hq.pos : { x: 200, y: 200 });
    const dist = hq ? distance(hq.pos, focus) : 0;
    const remote = dist > DIRECT_COMMAND_RADIUS_M;
    const delayTicks = remote ? Math.max(1, Math.ceil((dist - DIRECT_COMMAND_RADIUS_M) / 100)) : 0;
    const effectiveTick = obf.anchorTick + delayTicks;

    const wave: WaveDirective = {
      id,
      title: request.title.trim() || `排波次 #${this.waveSeq - 1}`,
      issuer: '排长指挥',
      createdTick: this.tick,
      effectiveTick,
      effectiveExactMs: obf.anchorExactMs,
      remote,
      delayTicks,
      effective: false,
      geometry: {
        barrages: request.geometry.barrages.map((b) =>
          b.type === 'circle'
            ? { type: 'circle', center: { ...b.center }, radiusM: b.radiusM }
            : { type: 'polygon', points: b.points.map((p) => ({ ...p })) },
        ),
        assaultArrows: request.geometry.assaultArrows.map((a) => a.map((p) => ({ ...p }))),
        aos: request.geometry.aos.map((ao) => ({ min: { ...ao.min }, max: { ...ao.max } })),
      },
    };

    this.waves.push(wave);
    this.stepCache.schedule(this.tick, {
      eventId: `wave-ev-${id}`,
      team: 'red',
      sourceTick: this.tick,
      targetTick: effectiveTick,
      secInTick: Math.ceil(obf.anchorExactMs / 1000),
      exactMs: obf.anchorExactMs,
      kind: 'g_field_wave',
      targetPos: { ...focus },
      payload: { type: 'g_field_wave', waveId: id, label: wave.title },
    });

    return cloneWave(wave);
  }

  async advanceTick(observer: Team): Promise<TickPlayback> {
    const nextTick = this.tick + 1;
    this.tick = nextTick;
    const baseline = this.snapshotFor(observer, 0);

    const keyframes: Record<number, UnitKeyframe[]> = {};
    for (const u of this.units) {
      keyframes[u.id] = [{ ms: 0, pos: { ...u.pos }, workProgress: u.workProgress }];
    }

    const changes: DiscreteChange[] = [];
    const log: PlaybackLogEntry[] = [];
    const shakeAtMs: number[] = [];
    const completedDirectiveIds: string[] = [];
    const effectiveWaveIds: string[] = [];

    // 回合初记录各单位当前状态用于 [0..16000] 分段演化
    const simPos = new Map<number, Vec2>();
    const lastMs = new Map<number, number>();
    const moveSpeed = new Map<number, number>();
    for (const u of this.units) {
      simPos.set(u.id, { ...u.pos });
      lastMs.set(u.id, 0);
      moveSpeed.set(u.id, WALK_SPEED_MPS);
    }

    const advanceUnitToMs = (u: UnitState, targetMs: number): void => {
      const prevMs = lastMs.get(u.id) ?? 0;
      if (targetMs <= prevMs || !u.alive) return;
      const dt = (targetMs - prevMs) / 1000;
      const cur = simPos.get(u.id)!;

      if (u.activity.type === 'move' && u.activity.path.length > 0) {
        let remain = dt * (moveSpeed.get(u.id) ?? WALK_SPEED_MPS);
        let pos = { ...cur };
        const path = u.activity.path;
        while (remain > 0 && path.length > 0) {
          const wp = path[0]!;
          const dx = wp.x - pos.x;
          const dy = wp.y - pos.y;
          const d = Math.hypot(dx, dy);
          if (d <= remain || d < 0.2) {
            pos = clampPos(wp);
            path.shift();
            remain -= d;
          } else {
            pos = clampPos({ x: pos.x + (dx / d) * remain, y: pos.y + (dy / d) * remain });
            remain = 0;
          }
        }
        simPos.set(u.id, pos);
        u.pos = { ...pos };
        if (path.length === 0) {
          u.activity = { type: 'idle' };
          changes.push({ ms: targetMs, type: 'unit_activity', unitId: u.id, activity: { type: 'idle' } });
        }
      } else if (u.activity.type === 'build') {
        u.workProgress = Math.min(1, u.workProgress + dt / 8);
      } else if (u.activity.type === 'fire' && u.ammo > 0) {
        const spent = Math.min(u.ammo, Math.max(1, Math.round(dt * 3)));
        if (spent > 0) {
          u.ammo -= spent;
          changes.push({ ms: targetMs, type: 'unit', unitId: u.id, field: 'ammo', value: u.ammo });
        }
      }

      keyframes[u.id]!.push({ ms: targetMs, pos: { ...u.pos }, workProgress: u.workProgress });
      lastMs.set(u.id, targetMs);
    };

    const due = this.stepCache.drainDue(nextTick);

    for (const ev of due) {
      // 先把所有单位演化到当前事件 exactMs
      for (const u of this.units) advanceUnitToMs(u, ev.exactMs);

      if (ev.payload.type === 'detonation') {
        const pos = ev.targetPos ?? { x: 320, y: 240 };
        const crater: TerrainDestruction = {
          id: `crater-${nextTick}-${this.eventSeq++}`,
          kind: 'crater',
          pos: { ...pos },
          radiusM: ev.payload.radiusM,
          createdTick: nextTick,
        };
        this.destruction.push(crater);
        changes.push({ ms: ev.exactMs, type: 'destruction', item: crater });
        shakeAtMs.push(ev.exactMs);
        log.push({ ms: ev.exactMs, text: `[${(ev.exactMs / 1000).toFixed(1)}s] ${ev.payload.label} @ (${pos.x}, ${pos.y})`, kind: 'detonation' });

        // 对爆心半径内单位造成按秒兑现伤亡与压制
        for (const u of this.units) {
          if (!u.alive) continue;
          const d = distance(u.pos, pos);
          if (d <= ev.payload.radiusM * 1.6) {
            const lost = Math.min(u.soldiers, d <= ev.payload.radiusM ? 2 : 1);
            if (lost > 0) {
              u.soldiers -= lost;
              u.suppression = Math.min(100, u.suppression + 35);
              changes.push({ ms: ev.exactMs, type: 'unit', unitId: u.id, field: 'soldiers', value: u.soldiers });
              changes.push({ ms: ev.exactMs, type: 'unit', unitId: u.id, field: 'suppression', value: u.suppression });
              const corpse: GroundObject = {
                id: `corpse-${nextTick}-${this.eventSeq++}`,
                kind: 'corpse',
                pos: { x: u.pos.x + 6, y: u.pos.y + 4 },
                label: `${u.name} 阵亡`,
              };
              this.groundObjects.push(corpse);
              changes.push({ ms: ev.exactMs, type: 'ground_object', item: corpse });
              if (u.soldiers === 0) {
                u.alive = false;
                changes.push({ ms: ev.exactMs, type: 'unit_alive', unitId: u.id, alive: false });
              }
            }
          }
        }
      } else if (ev.payload.type === 'task_command' || ev.payload.type === 'macro_stop') {
        const cmd: TacticalCommandJson = ev.payload.command;
        const u = this.units.find((x) => x.id === cmd.unit_id);
        if (u && u.alive) {
          this.applyCommandAtMs(u, cmd, ev.exactMs, moveSpeed, changes, nextTick);
        }
        if (ev.payload.type === 'macro_stop') {
          completedDirectiveIds.push(ev.payload.directiveId);
        }
        log.push({ ms: ev.exactMs, text: `[${(ev.exactMs / 1000).toFixed(1)}s] ${ev.payload.label}`, kind: ev.kind });
      } else if (ev.payload.type === 'g_field_wave') {
        const waveId = ev.payload.waveId;
        const w = this.waves.find((x) => x.id === waveId);
        if (w) {
          w.effective = true;
          effectiveWaveIds.push(w.id);
          // 炮击区第一处落弹生成弹坑
          const b0 = w.geometry.barrages[0];
          const hitPos = b0?.type === 'circle' ? b0.center : b0?.type === 'polygon' ? b0.points[0] : undefined;
          if (hitPos) {
            const crater: TerrainDestruction = {
              id: `wave-crater-${w.id}`,
              kind: 'scorch',
              pos: { ...hitPos },
              radiusM: b0?.type === 'circle' ? Math.min(28, b0.radiusM * 0.55) : 22,
              createdTick: nextTick,
            };
            this.destruction.push(crater);
            changes.push({ ms: ev.exactMs, type: 'destruction', item: crater });
            shakeAtMs.push(ev.exactMs);
          }
          log.push({ ms: ev.exactMs, text: `[${(ev.exactMs / 1000).toFixed(1)}s] G场生效：${w.title}`, kind: 'g_field_wave' });
        }
      }
    }

    // 全部单位演化至回合末 16000ms，期间按 4s 分段记录逐秒交火/弹耗跳字
    const checkpoints = [4000, 8000, 12000, TICK_MS];
    for (const cp of checkpoints) {
      for (const u of this.units) {
        if ((lastMs.get(u.id) ?? 0) < cp) {
          advanceUnitToMs(u, cp);
        }
      }
      // 简单交火：处于 fire 态或靠近敌方且未持枪停火
      for (const u of this.units) {
        if (!u.alive || u.discipline === 'hold_fire' || u.ammo <= 0) continue;
        const enemies = this.units.filter((e) => e.alive && e.team !== u.team && distance(u.pos, e.pos) <= 180);
        if (enemies.length > 0 && cp === 12000) {
          const target = enemies[0]!;
          const spent = Math.min(u.ammo, 12);
          u.ammo -= spent;
          changes.push({ ms: cp, type: 'unit', unitId: u.id, field: 'ammo', value: u.ammo });
          target.suppression = Math.min(100, target.suppression + 15);
          changes.push({ ms: cp, type: 'unit', unitId: target.id, field: 'suppression', value: target.suppression });
          log.push({ ms: cp, text: `[${(cp / 1000).toFixed(0)}s] ${u.name} 向 ${target.name} 短点射 (-${spent} 发)`, kind: 'fire' });
        }
      }
    }

    const final = this.snapshotFor(observer, TICK_MS);
    return {
      tick: nextTick,
      observer,
      baseline,
      keyframes,
      changes,
      log,
      shakeAtMs,
      completedDirectiveIds,
      effectiveWaveIds,
      final,
    };
  }

  private applyCommandAtMs(
    u: UnitState,
    cmd: TacticalCommandJson,
    ms: number,
    moveSpeed: Map<number, number>,
    changes: DiscreteChange[],
    tick: number,
  ): void {
    switch (cmd.kind) {
      case 'move_unit':
        moveSpeed.set(u.id, speedForStance(cmd.stance));
        u.activity = { type: 'move', path: cmd.waypoints.map((p) => ({ ...p })) };
        changes.push({ ms, type: 'unit_activity', unitId: u.id, activity: u.activity });
        break;
      case 'attack_pos':
        u.activity = { type: 'fire', target: { ...cmd.target } };
        changes.push({ ms, type: 'unit_activity', unitId: u.id, activity: u.activity });
        break;
      case 'attack_unit': {
        const tgt = this.units.find((x) => x.id === cmd.target_id);
        u.activity = { type: 'fire', target: tgt ? { ...tgt.pos } : { ...u.pos }, targetUnitId: cmd.target_id };
        changes.push({ ms, type: 'unit_activity', unitId: u.id, activity: u.activity });
        break;
      }
      case 'hold_unit':
        u.activity = { type: 'hold' };
        changes.push({ ms, type: 'unit_activity', unitId: u.id, activity: { type: 'hold' } });
        break;
      case 'set_fire_discipline':
        u.discipline = cmd.discipline;
        changes.push({ ms, type: 'unit_discipline', unitId: u.id, discipline: cmd.discipline });
        break;
      case 'pop_smoke': {
        const cloud: SmokeCloud = {
          id: `smoke-${tick}-${this.eventSeq++}`,
          pos: { ...cmd.target },
          radiusM: 28,
          createdTick: tick,
        };
        this.smoke.push(cloud);
        changes.push({ ms, type: 'smoke', item: cloud });
        break;
      }
      case 'build_work': {
        u.workProgress = 0;
        u.activity = { type: 'build', work: cmd.work, points: cmd.waypoints.map((p) => ({ ...p })) };
        changes.push({ ms, type: 'unit_activity', unitId: u.id, activity: u.activity });
        const pts = cmd.waypoints;
        const center = pts[0] ?? u.pos;
        const d: TerrainDestruction = {
          id: `work-${tick}-${this.eventSeq++}`,
          kind: cmd.work === 'zigzag_trench' ? 'trench' : 'foxhole',
          pos: { ...center },
          radiusM: 12,
          points: pts.map((p) => ({ ...p })),
          createdTick: tick,
        };
        this.destruction.push(d);
        changes.push({ ms, type: 'destruction', item: d });
        break;
      }
    }
  }

  async fastForwardTo(targetTick: number, observer: Team): Promise<TickPlayback[]> {
    const out: TickPlayback[] = [];
    const goal = Math.max(this.tick, Math.floor(targetTick));
    while (this.tick < goal) {
      out.push(await this.advanceTick(observer));
    }
    return out;
  }

  async lookahead(targetTick: number, observer: Team): Promise<ScheduledStepEvent[]> {
    return this.stepCache.lookahead(targetTick, observer);
  }

  async listScheduled(observer: Team): Promise<ScheduledStepEvent[]> {
    return this.stepCache.listForTeam(observer);
  }
}

export { TICK_SECONDS };
