use serde::{Deserialize, Serialize};

/// Four anatomical regions tracked per individual infantry soldier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyPartKind {
    Head = 0,
    Torso = 1,
    UpperLimbs = 2,
    LowerLimbs = 3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyRegionState {
    pub integrity: f32,
    pub cumulative_damage: f32,
}

impl Default for BodyRegionState {
    fn default() -> Self {
        Self {
            integrity: 1.0,
            cumulative_damage: 0.0,
        }
    }
}

/// Per-soldier accounting record (`士兵按个人记账`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SoldierRecord {
    pub soldier_id: u16,
    pub alive: bool,
    pub hp: f32,
    pub max_hp: f32,
    /// [Head, Torso, UpperLimbs, LowerLimbs]
    pub regions: [BodyRegionState; 4],
    pub bleeding_loss: f32,
    pub bleed_rate_per_tick: f32,
    pub suppression: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoldierHitOutcome {
    pub killed: bool,
    pub hp_loss: f32,
    pub part: BodyPartKind,
}

impl SoldierRecord {
    pub fn new_healthy(soldier_id: u16, max_hp: f32) -> Self {
        let m = max_hp.max(1.0);
        Self {
            soldier_id,
            alive: true,
            hp: m,
            max_hp: m,
            regions: [
                BodyRegionState::default(),
                BodyRegionState::default(),
                BodyRegionState::default(),
                BodyRegionState::default(),
            ],
            bleeding_loss: 0.0,
            bleed_rate_per_tick: 0.0,
            suppression: 0.0,
        }
    }

    #[inline]
    pub fn select_hit_part(det_hash: u64) -> BodyPartKind {
        match det_hash % 100 {
            0..=11 => BodyPartKind::Head,
            12..=55 => BodyPartKind::Torso,
            56..=77 => BodyPartKind::UpperLimbs,
            _ => BodyPartKind::LowerLimbs,
        }
    }

    pub fn apply_hit(
        &mut self,
        part: BodyPartKind,
        raw_damage: f32,
        supp_add: f32,
    ) -> SoldierHitOutcome {
        self.suppression = (self.suppression + supp_add.max(0.0)).clamp(0.0, 10.0);
        if !self.alive || raw_damage <= 0.0 {
            return SoldierHitOutcome {
                killed: false,
                hp_loss: 0.0,
                part,
            };
        }

        let (hp_mult, region_scale, bleed_mult) = match part {
            BodyPartKind::Head => (1.85, 2.2, 0.45),
            BodyPartKind::Torso => (1.25, 1.4, 0.35),
            BodyPartKind::UpperLimbs => (0.70, 1.1, 0.14),
            BodyPartKind::LowerLimbs => (0.75, 1.1, 0.18),
        };

        let hp_loss = (raw_damage * hp_mult).min(self.hp);
        let prev_hp = self.hp;
        self.hp = (self.hp - hp_loss).max(0.0);

        let idx = part as usize;
        let region_loss = (raw_damage / self.max_hp.max(1.0)) * region_scale;
        self.regions[idx].integrity = (self.regions[idx].integrity - region_loss).clamp(0.0, 1.0);
        self.regions[idx].cumulative_damage += raw_damage;
        self.bleed_rate_per_tick += raw_damage * bleed_mult;

        let fatal_region = self.regions[BodyPartKind::Head as usize].integrity <= 0.0
            || self.regions[BodyPartKind::Torso as usize].integrity <= 0.0;
        let mut killed = false;
        if self.hp <= 0.0 || fatal_region {
            self.alive = false;
            self.hp = 0.0;
            self.bleed_rate_per_tick = 0.0;
            killed = true;
        }

        SoldierHitOutcome {
            killed,
            hp_loss: (prev_hp - self.hp).max(0.0),
            part,
        }
    }

    /// Advances 16s bleeding for wounded soldiers. Returns `true` if soldier bled out this tick.
    pub fn tick_bleeding_16s(&mut self) -> bool {
        if !self.alive || self.bleed_rate_per_tick <= 0.0 {
            return false;
        }
        self.bleeding_loss += self.bleed_rate_per_tick;
        self.hp = (self.hp - self.bleed_rate_per_tick * 0.35).max(0.0);
        if self.bleeding_loss >= 100.0 || self.hp <= 0.0 {
            self.alive = false;
            self.hp = 0.0;
            self.bleed_rate_per_tick = 0.0;
            return true;
        }
        false
    }
}

/// 4-facing Rolled Homogeneous Armor (mm RHA) for armored vehicles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArmorFacing {
    Front = 0,
    Side = 1,
    Rear = 2,
    Top = 3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArmorProfileRha {
    pub front_mm: f32,
    pub side_mm: f32,
    pub rear_mm: f32,
    pub top_mm: f32,
}

impl ArmorProfileRha {
    pub fn from_base_mm(base_mm: f32) -> Self {
        let b = base_mm.max(0.0);
        Self {
            front_mm: b,
            side_mm: b * 0.60,
            rear_mm: b * 0.35,
            top_mm: b * 0.25,
        }
    }

    #[inline]
    pub fn mm_for(&self, facing: ArmorFacing) -> f32 {
        match facing {
            ArmorFacing::Front => self.front_mm,
            ArmorFacing::Side => self.side_mm,
            ArmorFacing::Rear => self.rear_mm,
            ArmorFacing::Top => self.top_mm,
        }
    }
}

/// 8 core vehicle subsystems damaged internally upon armor defeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VehicleSubsystemKind {
    Engine = 0,
    Tracks = 1,
    MainGun = 2,
    TurretRing = 3,
    Optics = 4,
    Radio = 5,
    AmmoRack = 6,
    FuelTank = 7,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VehicleDamageState {
    pub armor_rha: ArmorProfileRha,
    /// [Engine, Tracks, MainGun, TurretRing, Optics, Radio, AmmoRack, FuelTank] in 0.0..=1.0
    pub subsystems: [f32; 8],
    pub crew_alive: u16,
    pub crew_max: u16,
    pub ammo_cooked_off: bool,
    pub burning: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VehicleImpactOutcome {
    pub facing: ArmorFacing,
    pub armor_mm: f32,
    pub penetrated: bool,
    pub internal_spall_energy: f32,
    /// Hard iron law: tanks NEVER overpenetrate; exit flux behind vehicle is always 0.0.
    pub transmitted_flux: f32,
    pub hull_hp_loss: f32,
    pub crew_killed: u16,
    pub catastrophic_kill: bool,
}

/// Resolves which vehicle armor facing receives the strike.
/// `incoming_dir` is the projectile travel vector `(ux, uy)` from shooter to target.
pub fn select_armor_facing(
    incoming_dir: [f32; 2],
    vehicle_facing_deg: f32,
    is_overhead: bool,
) -> ArmorFacing {
    if is_overhead {
        return ArmorFacing::Top;
    }
    let len = (incoming_dir[0] * incoming_dir[0] + incoming_dir[1] * incoming_dir[1]).sqrt();
    if len <= 1e-6 {
        return ArmorFacing::Front;
    }
    let toward_shooter = [-incoming_dir[0] / len, -incoming_dir[1] / len];
    let rad = vehicle_facing_deg.to_radians();
    let fwd = [rad.cos(), rad.sin()];
    let dot = fwd[0] * toward_shooter[0] + fwd[1] * toward_shooter[1];
    if dot >= 0.5 {
        ArmorFacing::Front
    } else if dot <= -0.5 {
        ArmorFacing::Rear
    } else {
        ArmorFacing::Side
    }
}

impl VehicleDamageState {
    pub fn new(armor_rha: ArmorProfileRha, crew_count: u16) -> Self {
        let crew = crew_count.max(1);
        Self {
            armor_rha,
            subsystems: [1.0; 8],
            crew_alive: crew,
            crew_max: crew,
            ammo_cooked_off: false,
            burning: false,
        }
    }

    /// Resolves ballistic impact against 4-facing mm RHA.
    /// Zero overpenetration: any excess energy after armor defeat converts 100% into internal spalling.
    pub fn apply_ballistic_impact(
        &mut self,
        facing: ArmorFacing,
        armor_pen_mm: f32,
        impact_energy: f32,
        det_hash: u64,
    ) -> VehicleImpactOutcome {
        let plate_mm = self.armor_rha.mm_for(facing).max(0.0);
        if armor_pen_mm <= plate_mm {
            // Deflected / non-penetration: zero spall inside hull, minor external optics/track shock on heavy hits.
            if impact_energy >= 25.0 {
                let wear = ((impact_energy - 20.0) * 0.002).min(0.12);
                self.subsystems[VehicleSubsystemKind::Optics as usize] =
                    (self.subsystems[VehicleSubsystemKind::Optics as usize] - wear).max(0.0);
                self.subsystems[VehicleSubsystemKind::Tracks as usize] =
                    (self.subsystems[VehicleSubsystemKind::Tracks as usize] - wear * 0.6).max(0.0);
            }
            return VehicleImpactOutcome {
                facing,
                armor_mm: plate_mm,
                penetrated: false,
                internal_spall_energy: 0.0,
                transmitted_flux: 0.0,
                hull_hp_loss: (impact_energy * 0.04).min(2.0),
                crew_killed: 0,
                catastrophic_kill: false,
            };
        }

        // Penetration! Excess kinetic energy converts 100% into internal spalling against subsystems & crew.
        let excess_ratio = ((armor_pen_mm - plate_mm) / armor_pen_mm.max(1.0)).clamp(0.08, 1.0);
        let spall_energy = impact_energy.max(1.0) * (0.45 + 1.35 * excess_ratio);

        let candidate_table: [VehicleSubsystemKind; 4] = match facing {
            ArmorFacing::Front => [
                VehicleSubsystemKind::TurretRing,
                VehicleSubsystemKind::MainGun,
                VehicleSubsystemKind::Optics,
                VehicleSubsystemKind::AmmoRack,
            ],
            ArmorFacing::Side => [
                VehicleSubsystemKind::AmmoRack,
                VehicleSubsystemKind::Tracks,
                VehicleSubsystemKind::Radio,
                VehicleSubsystemKind::TurretRing,
            ],
            ArmorFacing::Rear => [
                VehicleSubsystemKind::Engine,
                VehicleSubsystemKind::FuelTank,
                VehicleSubsystemKind::Tracks,
                VehicleSubsystemKind::Radio,
            ],
            ArmorFacing::Top => [
                VehicleSubsystemKind::TurretRing,
                VehicleSubsystemKind::Engine,
                VehicleSubsystemKind::AmmoRack,
                VehicleSubsystemKind::Optics,
            ],
        };

        let primary_slot = (det_hash as usize) % 4;
        let secondary_slot = ((det_hash >> 8) as usize + 1) % 4;
        let tertiary_slot = ((det_hash >> 16) as usize + 2) % 4;
        let spall_dmg_norm = (spall_energy / 85.0).clamp(0.08, 1.0);

        for (slot, weight) in [
            (candidate_table[primary_slot], 0.55),
            (candidate_table[secondary_slot], 0.30),
            (candidate_table[tertiary_slot], 0.15),
        ] {
            let idx = slot as usize;
            self.subsystems[idx] = (self.subsystems[idx] - spall_dmg_norm * weight * 1.4).max(0.0);
        }

        if self.subsystems[VehicleSubsystemKind::FuelTank as usize] <= 0.25 {
            self.burning = true;
        }

        let mut crew_killed = if spall_energy >= 28.0 {
            ((spall_energy / 42.0).floor() as u16).max(1)
        } else {
            0
        };
        crew_killed = crew_killed.min(self.crew_alive);
        self.crew_alive = self.crew_alive.saturating_sub(crew_killed);

        let mut catastrophic_kill = false;
        let mut hull_hp_loss = spall_energy * 0.85;
        if self.subsystems[VehicleSubsystemKind::AmmoRack as usize] <= 0.15 {
            self.ammo_cooked_off = true;
            self.burning = true;
            crew_killed = crew_killed.saturating_add(self.crew_alive);
            self.crew_alive = 0;
            catastrophic_kill = true;
            hull_hp_loss += 500.0;
        } else if self.crew_alive == 0 {
            catastrophic_kill = true;
        }

        VehicleImpactOutcome {
            facing,
            armor_mm: plate_mm,
            penetrated: true,
            internal_spall_energy: spall_energy,
            transmitted_flux: 0.0,
            hull_hp_loss,
            crew_killed,
            catastrophic_kill,
        }
    }
}
