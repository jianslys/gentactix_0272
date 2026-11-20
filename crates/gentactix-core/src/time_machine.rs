//! `time_machine` module: External Pseudo-Asynchronous Scripted Timeline and Time Machine
//! 遵循 GTX-SPEC-PSEUDO-ASYNC-TIMEMACHINE 与 V6.0 法典第一章铁律：
//! 1. 唯一外置时间机：对内拦单跨回合错峰调度，对外只读粒子线性插值与 visibility=0 预渲染环；
//! 2. 绝对零业务污染：只算时间偏移与毫秒戳 exact_ms，绝不修改底层原生物理伤害数值与公式；
//! 3. 双哈希混淆与五张 16 槽品类时间常量表：SplitMix64(abs_sec) 结合 FNV1a_64(unit_name)。

use serde::{Deserialize, Serialize};

/// 64 位 SplitMix64 散列函数
#[inline]
pub fn splitmix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// 64 位 FNV-1a 编译期/常量字符串哈希
#[inline]
pub fn fnv1a_64(s: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &b in s.as_bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// 极简确定性双哈希混淆公式：
/// M = SplitMix64(SplitMix64(t_abs) ^ FNV1a_64(unit_name).rotate_left(27))
#[inline]
pub fn mix_time_unit_hash(abs_sec: u64, unit_name: &str) -> u64 {
    let h_sec = splitmix64(abs_sec);
    let h_unit = fnv1a_64(unit_name);
    splitmix64(h_sec ^ h_unit.rotate_left(27))
}

/// 时间机分品类枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeCategory {
    Bullet,
    Shell,
    Command,
    Event,
    Movement,
}

impl TimeCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bullet => "BULLET",
            Self::Shell => "SHELL",
            Self::Command => "COMMAND",
            Self::Event => "EVENT",
            Self::Movement => "MOVEMENT",
        }
    }
}

/// 16 槽时间表单元：(turn_skip, tau_phase [1..=16], exact_ms [1..=16000])
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeLutEntry {
    pub turn_skip: u32,
    pub tau_phase: u32,
    pub exact_ms: u32,
}

/// 1. LUT_BULLET：子弹 / 轻武器时间表 (准备延迟最短，毫秒紧凑密集)
pub const LUT_BULLET: [TimeLutEntry; 16] = [
    TimeLutEntry { turn_skip: 0, tau_phase: 1, exact_ms: 320 },
    TimeLutEntry { turn_skip: 0, tau_phase: 1, exact_ms: 580 },
    TimeLutEntry { turn_skip: 0, tau_phase: 1, exact_ms: 840 },
    TimeLutEntry { turn_skip: 0, tau_phase: 2, exact_ms: 1120 },
    TimeLutEntry { turn_skip: 0, tau_phase: 2, exact_ms: 1450 },
    TimeLutEntry { turn_skip: 0, tau_phase: 2, exact_ms: 1780 },
    TimeLutEntry { turn_skip: 0, tau_phase: 2, exact_ms: 2010 },
    TimeLutEntry { turn_skip: 0, tau_phase: 3, exact_ms: 2320 },
    TimeLutEntry { turn_skip: 0, tau_phase: 3, exact_ms: 2650 },
    TimeLutEntry { turn_skip: 0, tau_phase: 3, exact_ms: 2980 },
    TimeLutEntry { turn_skip: 0, tau_phase: 3, exact_ms: 3210 },
    TimeLutEntry { turn_skip: 0, tau_phase: 4, exact_ms: 3450 },
    TimeLutEntry { turn_skip: 0, tau_phase: 4, exact_ms: 3680 },
    TimeLutEntry { turn_skip: 0, tau_phase: 4, exact_ms: 3910 },
    TimeLutEntry { turn_skip: 0, tau_phase: 4, exact_ms: 4150 },
    TimeLutEntry { turn_skip: 0, tau_phase: 4, exact_ms: 4380 },
];

/// 2. LUT_SHELL：炮弹 / 重武器时间表 (准备延迟深，单发落弹跨度大)
pub const LUT_SHELL: [TimeLutEntry; 16] = [
    TimeLutEntry { turn_skip: 1, tau_phase: 4, exact_ms: 4200 },
    TimeLutEntry { turn_skip: 1, tau_phase: 5, exact_ms: 5350 },
    TimeLutEntry { turn_skip: 1, tau_phase: 6, exact_ms: 6480 },
    TimeLutEntry { turn_skip: 1, tau_phase: 7, exact_ms: 7620 },
    TimeLutEntry { turn_skip: 1, tau_phase: 8, exact_ms: 8740 },
    TimeLutEntry { turn_skip: 1, tau_phase: 9, exact_ms: 9860 },
    TimeLutEntry { turn_skip: 1, tau_phase: 10, exact_ms: 10950 },
    TimeLutEntry { turn_skip: 1, tau_phase: 11, exact_ms: 11800 },
    TimeLutEntry { turn_skip: 1, tau_phase: 12, exact_ms: 12650 },
    TimeLutEntry { turn_skip: 1, tau_phase: 13, exact_ms: 13420 },
    TimeLutEntry { turn_skip: 1, tau_phase: 14, exact_ms: 14190 },
    TimeLutEntry { turn_skip: 1, tau_phase: 15, exact_ms: 14920 },
    TimeLutEntry { turn_skip: 1, tau_phase: 16, exact_ms: 15680 },
    TimeLutEntry { turn_skip: 1, tau_phase: 5, exact_ms: 5720 },
    TimeLutEntry { turn_skip: 1, tau_phase: 8, exact_ms: 8910 },
    TimeLutEntry { turn_skip: 1, tau_phase: 11, exact_ms: 11950 },
];

/// 3. LUT_COMMAND：指挥指令与 AO 时间表 (发报通联至 G0 生效)
pub const LUT_COMMAND: [TimeLutEntry; 16] = [
    TimeLutEntry { turn_skip: 0, tau_phase: 3, exact_ms: 3100 },
    TimeLutEntry { turn_skip: 0, tau_phase: 4, exact_ms: 4250 },
    TimeLutEntry { turn_skip: 0, tau_phase: 5, exact_ms: 5400 },
    TimeLutEntry { turn_skip: 0, tau_phase: 6, exact_ms: 6600 },
    TimeLutEntry { turn_skip: 0, tau_phase: 7, exact_ms: 7800 },
    TimeLutEntry { turn_skip: 0, tau_phase: 8, exact_ms: 8950 },
    TimeLutEntry { turn_skip: 0, tau_phase: 9, exact_ms: 9980 },
    TimeLutEntry { turn_skip: 0, tau_phase: 10, exact_ms: 10850 },
    TimeLutEntry { turn_skip: 0, tau_phase: 11, exact_ms: 11720 },
    TimeLutEntry { turn_skip: 0, tau_phase: 12, exact_ms: 12550 },
    TimeLutEntry { turn_skip: 1, tau_phase: 2, exact_ms: 2100 },
    TimeLutEntry { turn_skip: 1, tau_phase: 3, exact_ms: 3300 },
    TimeLutEntry { turn_skip: 1, tau_phase: 4, exact_ms: 4500 },
    TimeLutEntry { turn_skip: 0, tau_phase: 4, exact_ms: 4680 },
    TimeLutEntry { turn_skip: 0, tau_phase: 7, exact_ms: 7420 },
    TimeLutEntry { turn_skip: 0, tau_phase: 9, exact_ms: 9650 },
];

/// 4. LUT_EVENT：九大工程与爆破时间表 (工兵作业与起爆合闸)
pub const LUT_EVENT: [TimeLutEntry; 16] = [
    TimeLutEntry { turn_skip: 0, tau_phase: 5, exact_ms: 5200 },
    TimeLutEntry { turn_skip: 0, tau_phase: 6, exact_ms: 6400 },
    TimeLutEntry { turn_skip: 0, tau_phase: 7, exact_ms: 7700 },
    TimeLutEntry { turn_skip: 0, tau_phase: 8, exact_ms: 8900 },
    TimeLutEntry { turn_skip: 0, tau_phase: 9, exact_ms: 9950 },
    TimeLutEntry { turn_skip: 0, tau_phase: 10, exact_ms: 10880 },
    TimeLutEntry { turn_skip: 0, tau_phase: 11, exact_ms: 11750 },
    TimeLutEntry { turn_skip: 0, tau_phase: 12, exact_ms: 12410 },
    TimeLutEntry { turn_skip: 0, tau_phase: 13, exact_ms: 13200 },
    TimeLutEntry { turn_skip: 0, tau_phase: 14, exact_ms: 14100 },
    TimeLutEntry { turn_skip: 0, tau_phase: 15, exact_ms: 15050 },
    TimeLutEntry { turn_skip: 0, tau_phase: 16, exact_ms: 15900 },
    TimeLutEntry { turn_skip: 1, tau_phase: 4, exact_ms: 4300 },
    TimeLutEntry { turn_skip: 1, tau_phase: 6, exact_ms: 6700 },
    TimeLutEntry { turn_skip: 1, tau_phase: 8, exact_ms: 8850 },
    TimeLutEntry { turn_skip: 0, tau_phase: 12, exact_ms: 12890 },
];

/// 5. LUT_MOVEMENT：移动与撤退时间表 (吹哨整队与起步切入)
pub const LUT_MOVEMENT: [TimeLutEntry; 16] = [
    TimeLutEntry { turn_skip: 0, tau_phase: 1, exact_ms: 1100 },
    TimeLutEntry { turn_skip: 0, tau_phase: 1, exact_ms: 1450 },
    TimeLutEntry { turn_skip: 0, tau_phase: 2, exact_ms: 2200 },
    TimeLutEntry { turn_skip: 0, tau_phase: 2, exact_ms: 2600 },
    TimeLutEntry { turn_skip: 0, tau_phase: 3, exact_ms: 3150 },
    TimeLutEntry { turn_skip: 0, tau_phase: 3, exact_ms: 3550 },
    TimeLutEntry { turn_skip: 0, tau_phase: 4, exact_ms: 4100 },
    TimeLutEntry { turn_skip: 0, tau_phase: 4, exact_ms: 4500 },
    TimeLutEntry { turn_skip: 0, tau_phase: 5, exact_ms: 5150 },
    TimeLutEntry { turn_skip: 0, tau_phase: 5, exact_ms: 5650 },
    TimeLutEntry { turn_skip: 0, tau_phase: 6, exact_ms: 6100 },
    TimeLutEntry { turn_skip: 0, tau_phase: 6, exact_ms: 6600 },
    TimeLutEntry { turn_skip: 0, tau_phase: 2, exact_ms: 2450 },
    TimeLutEntry { turn_skip: 0, tau_phase: 3, exact_ms: 3750 },
    TimeLutEntry { turn_skip: 0, tau_phase: 4, exact_ms: 4850 },
    TimeLutEntry { turn_skip: 0, tau_phase: 5, exact_ms: 5900 },
];

/// 查分品类时间表：输入品类与 64 位混淆值，返回 (turn_skip, tau_phase, exact_ms)
#[inline]
pub fn query_time_lut(category: TimeCategory, hash_m: u64) -> TimeLutEntry {
    let slot = (hash_m & 0x0F) as usize;
    match category {
        TimeCategory::Bullet => LUT_BULLET[slot],
        TimeCategory::Shell => LUT_SHELL[slot],
        TimeCategory::Command => LUT_COMMAND[slot],
        TimeCategory::Event => LUT_EVENT[slot],
        TimeCategory::Movement => LUT_MOVEMENT[slot],
    }
}

/// 挂起操作票据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOpTicket {
    pub ticket_id: u64,
    pub category: TimeCategory,
    pub unit_name: String,
    pub issued_abs_sec: u64,
    pub activate_tick: u64,
    pub activate_ms: u32,
    pub min_duration_s: u32,
    pub raw_op_payload: serde_json::Value,
}

/// 时间机挂起排队调度器 (TimeMachinePendingQueue)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimeMachinePendingQueue {
    pub next_ticket_id: u64,
    pub tickets: Vec<PendingOpTicket>,
}

impl TimeMachinePendingQueue {
    pub fn new() -> Self {
        Self {
            next_ticket_id: 1,
            tickets: Vec::new(),
        }
    }

    /// 拦截 #0 当回合指令，赋予跨回合准备时间并入队
    pub fn enqueue_operation(
        &mut self,
        current_tick: u64,
        t_cmd_sec: u32, // 1..=16
        category: TimeCategory,
        unit_name: &str,
        min_duration_s: u32,
        raw_op_payload: serde_json::Value,
    ) -> PendingOpTicket {
        let t_cmd_clamped = t_cmd_sec.clamp(1, 16);
        let issued_abs_sec = current_tick * 16 + (t_cmd_clamped as u64);
        let hash_m = mix_time_unit_hash(issued_abs_sec, unit_name);
        let lut_entry = query_time_lut(category, hash_m);

        // 严格遵循 SPEC 公式：
        // T_prep = (16 - t_cmd) + 16 * k_turn_skip + tau_phase
        // 目标激活 tick = current_tick + 1 + turn_skip
        let activate_tick = current_tick + 1 + (lut_entry.turn_skip as u64);
        let activate_ms = lut_entry.exact_ms;

        let ticket = PendingOpTicket {
            ticket_id: self.next_ticket_id,
            category,
            unit_name: unit_name.to_string(),
            issued_abs_sec,
            activate_tick,
            activate_ms,
            min_duration_s: min_duration_s.max(16),
            raw_op_payload,
        };

        self.next_ticket_id += 1;
        self.tickets.push(ticket.clone());
        ticket
    }

    /// 在回合交界处提取落入当前 tick 窗口内的所有有效操作
    pub fn drain_due_operations(&mut self, current_tick: u64) -> Vec<PendingOpTicket> {
        let mut due = Vec::new();
        let mut remaining = Vec::with_capacity(self.tickets.len());
        for t in self.tickets.drain(..) {
            if t.activate_tick <= current_tick {
                due.push(t);
            } else {
                remaining.push(t);
            }
        }
        self.tickets = remaining;
        due
    }
}

/// 时间机外发拉格朗日粒子轨线节点 (用于前端 0..16000ms 纯只读分段关键帧与断行截断插值)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticleTrackNode {
    pub piece_id: String,
    pub team: String,
    pub pos_start: [f32; 2],
    pub pos_end: [f32; 2],
    pub initial_visibility: u8,
    pub is_prerender_skirt: bool,
    #[serde(default)]
    pub keyframes: Vec<(u32, [f32; 2])>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_exact_ms: Option<u32>,
}

/// 回合初 t=0s 单位置信基线 (防止前端 t=0s 提前剧透 t=16s 终局数值)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitTickBaseline {
    pub piece_id: String,
    pub team: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hp_start: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub men_start: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ammo_start: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppression_start: Option<f32>,
}

/// 时间机定点放片事件 (携带原生物理伤害数值，绝不削减)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackEvent {
    pub exact_ms: u32,
    pub category: TimeCategory,
    pub event_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_piece_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_piece_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_hp_loss: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_supp_add: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hp_delta: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub casualties: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ammo_spent: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terrain_cell: Option<[i32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directive_id: Option<String>,
}

/// 时间机对外载荷 (TimeMachineFramePayload)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeMachineFramePayload {
    pub tick: u64,
    pub window_ms: [u32; 2],
    #[serde(default)]
    pub unit_baselines: Vec<UnitTickBaseline>,
    pub particle_tracks: Vec<ParticleTrackNode>,
    pub playback_events: Vec<PlaybackEvent>,
}

impl TimeMachineFramePayload {
    pub fn empty(tick: u64) -> Self {
        Self {
            tick,
            window_ms: [0, 16000],
            unit_baselines: Vec::new(),
            particle_tracks: Vec::new(),
            playback_events: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_category_luts_determinism() {
        // 1. 测试 5 组固定输入，确认比特级确定性，无随机数漂移
        let t_abs = 23416u64;
        let unit_name = "arty_red_01";
        let h1 = mix_time_unit_hash(t_abs, unit_name);
        let h2 = mix_time_unit_hash(t_abs, unit_name);
        assert_eq!(h1, h2, "双哈希混淆必须完全确定");

        // 查五张品类表
        let entry_bullet = query_time_lut(TimeCategory::Bullet, h1);
        let entry_shell = query_time_lut(TimeCategory::Shell, h1);
        let entry_cmd = query_time_lut(TimeCategory::Command, h1);
        let entry_event = query_time_lut(TimeCategory::Event, h1);
        let entry_move = query_time_lut(TimeCategory::Movement, h1);

        assert!(entry_bullet.exact_ms <= 4500, "子弹毫秒偏移紧凑");
        assert!(entry_shell.turn_skip >= 1, "重炮具有跨回合准备延迟");
        assert!(entry_move.exact_ms <= 7000, "移动整队起步在前半程");
        assert!(entry_cmd.tau_phase >= 1 && entry_cmd.tau_phase <= 16);
        assert!(entry_event.tau_phase >= 1 && entry_event.tau_phase <= 16);
    }

    #[test]
    fn test_turn_zero_freeze_and_cross_turn_window_slicing() {
        let mut queue = TimeMachinePendingQueue::new();
        // 在 #0 回合第 5 秒下令
        let ticket = queue.enqueue_operation(
            0,
            5,
            TimeCategory::Shell,
            "howitzer_squad_1",
            30,
            serde_json::json!({"op": "fire", "target": "blue_sq_1"}),
        );

        // #0 当回合：没有到期任务 (零改数)
        let due_t0 = queue.drain_due_operations(0);
        assert!(due_t0.is_empty(), "#0 当回合内绝对不触发结算");

        // ticket 激活回合应为 0 + 1 + turn_skip >= 1
        assert!(ticket.activate_tick >= 1, "指令跨回合延期生效");

        // 到达激活回合交界
        let due_act = queue.drain_due_operations(ticket.activate_tick);
        assert_eq!(due_act.len(), 1, "到期回合交界切段喂数");
        assert_eq!(due_act[0].ticket_id, ticket.ticket_id);
    }

    #[test]
    fn test_exact_ms_unaltered_physics_values() {
        // 验证原装 exact_hp_loss / exact_supp_add 零篡改原封不动
        let event = PlaybackEvent {
            exact_ms: 8740,
            category: TimeCategory::Shell,
            event_kind: "shell_impact".into(),
            source_piece_id: Some("arty_red_01".into()),
            target_piece_id: Some("tank_blue_07".into()),
            exact_hp_loss: Some(34),
            exact_supp_add: Some(1.25),
            hp_delta: Some(34.0),
            casualties: Some(1),
            ammo_spent: Some(1),
            terrain_cell: None,
            entity_id: None,
            directive_id: None,
        };

        let json_val = serde_json::to_value(&event).unwrap();
        assert_eq!(json_val["exact_hp_loss"], 34, "原生物理伤害数值绝不被修改");
        assert_eq!(json_val["exact_supp_add"], 1.25, "原生物理压制数值绝不被修改");
        assert_eq!(json_val["exact_ms"], 8740);
    }

    #[test]
    fn test_prerender_skirt_visibility_zero() {
        // 验证预渲染缓冲环标记
        let track = ParticleTrackNode {
            piece_id: "skirt_enemy_01".into(),
            team: "blue".into(),
            pos_start: [30.0, 40.0],
            pos_end: [30.5, 40.2],
            initial_visibility: 0,
            is_prerender_skirt: true,
            keyframes: vec![(0, [30.0, 40.0]), (16000, [30.5, 40.2])],
            stop_exact_ms: None,
        };

        assert_eq!(track.initial_visibility, 0, "预渲染缓冲环初始必须隐身");
        assert!(track.is_prerender_skirt, "必须标记为预渲染缓冲环粒子");
    }
}
