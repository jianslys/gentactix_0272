//! Single-source item catalog loader, structural validator, O(1) index,
//! and modular weapon/mount/ammo/fuze attack-profile resolver.
//!
//! Enforces core ballistic & itemization canons:
//! 1. Weapons NEVER carry a hard `range_m` cutoff (distance only applies decay).
//! 2. Ammunition items own damage values & damage-type profiles (6-channel array).
//! 3. Fuzes are independent items modifying shell detonation state & effects.
//! 4. GPMG mounts are independent items toggling heavy-weapon/deployment/RPM traits.

use crate::units_ecs::DamageType;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

pub const EMBEDDED_ITEMS_CATALOG_JSON: &str = include_str!("../../../data/items_catalog.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemAttr {
    pub key: String,
    pub label: String,
    pub val: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogItemRaw {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub attrs: Vec<ItemAttr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogFile {
    pub version: String,
    pub schema: String,
    #[serde(default)]
    pub description: String,
    pub items: Vec<CatalogItemRaw>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MountModifiers {
    #[serde(default)]
    pub is_heavy_weapon: Option<bool>,
    #[serde(default)]
    pub deployable: Option<bool>,
    #[serde(default)]
    pub deploy_min: Option<f32>,
    #[serde(default)]
    pub combat_rpm_override: Option<f32>,
    #[serde(default)]
    pub base_accuracy_override: Option<f32>,
    #[serde(default)]
    pub base_accuracy_mult: Option<f32>,
    #[serde(default)]
    pub acc_decay_coeff_override: Option<f32>,
    #[serde(default)]
    pub acc_decay_mult: Option<f32>,
    #[serde(default)]
    pub optics_zoom: Option<f32>,
    #[serde(default)]
    pub crew_min: Option<u16>,
    #[serde(default)]
    pub crew_optimal: Option<u16>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FuzeEffectMod {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub anti_personnel_mult: Option<f32>,
    #[serde(default)]
    pub suppression_mult: Option<f32>,
    #[serde(default)]
    pub anti_soft_mult: Option<f32>,
    #[serde(default)]
    pub anti_armor_mult: Option<f32>,
    #[serde(default)]
    pub destructive_mult: Option<f32>,
    #[serde(default)]
    pub incendiary_mult: Option<f32>,
    #[serde(default)]
    pub frag_radius_mult: Option<f32>,
    #[serde(default)]
    pub ignore_trench_cover: Option<f32>,
    #[serde(default)]
    pub post_armor_spall_kill: Option<bool>,
    #[serde(default)]
    pub fallback_impact_detonate: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledWeapon {
    pub caliber: String,
    pub compatible_ammo: Vec<String>,
    pub compatible_mounts: Vec<String>,
    pub feed_type: String,
    pub base_rpm: f32,
    pub combat_rpm: f32,
    pub base_accuracy: f32,
    pub acc_decay_coeff: f32,
    pub dmg_decay_coeff: f32,
    pub is_heavy_weapon: bool,
    pub crew_min: u16,
    pub crew_optimal: u16,
    pub indirect: bool,
    pub direct_capable: bool,
    pub deployable: bool,
    pub deploy_min: f32,
    pub towable: bool,
    pub is_placeholder: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledAmmo {
    pub caliber: String,
    /// Damage channels indexed by `DamageType`:
    /// [0]=AntiPersonnel, [1]=Suppression, [2]=AntiSoft, [3]=AntiArmor, [4]=Destructive, [5]=Incendiary
    pub damage: [f32; 6],
    pub armor_pen_mm: f32,
    pub frag_radius_m: f32,
    pub requires_fuze: bool,
    pub compatible_fuzes: Vec<String>,
    pub no_fuze_behavior: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledFuze {
    pub fuze_mode: String,
    pub delay_s: f32,
    pub timer_max_s: f32,
    pub compatible_calibers: Vec<String>,
    pub effect_mod: FuzeEffectMod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledMount {
    pub compatible_weapons: Vec<String>,
    pub modifiers: MountModifiers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledItem {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub category: String,
    pub weight_kg: f32,
    pub grid_w: u16,
    pub grid_h: u16,
    pub stack_max: u16,
    pub can_hand: bool,
    pub attrs: Vec<ItemAttr>,
    pub weapon: Option<CompiledWeapon>,
    pub ammo: Option<CompiledAmmo>,
    pub fuze: Option<CompiledFuze>,
    pub mount: Option<CompiledMount>,
}

impl CompiledItem {
    pub fn attr(&self, key: &str) -> Option<&Value> {
        self.attrs.iter().find(|a| a.key == key).map(|a| &a.val)
    }

    pub fn attr_f32(&self, key: &str) -> Option<f32> {
        self.attr(key).and_then(|v| v.as_f64()).map(|n| n as f32)
    }

    pub fn attr_u16(&self, key: &str) -> Option<u16> {
        self.attr(key)
            .and_then(|v| v.as_u64())
            .and_then(|n| u16::try_from(n).ok())
    }

    pub fn attr_bool(&self, key: &str) -> Option<bool> {
        self.attr(key).and_then(|v| v.as_bool())
    }

    pub fn attr_str(&self, key: &str) -> Option<&str> {
        self.attr(key).and_then(|v| v.as_str())
    }

    pub fn attr_str_list(&self, key: &str) -> Vec<String> {
        match self.attr(key) {
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedAttackProfile {
    pub weapon_id: String,
    pub ammo_id: String,
    pub fuze_id: Option<String>,
    pub mount_ids: Vec<String>,
    pub base_rpm: f32,
    pub combat_rpm: f32,
    pub base_accuracy: f32,
    pub acc_decay_coeff: f32,
    pub dmg_decay_coeff: f32,
    pub is_heavy_weapon: bool,
    pub deployable: bool,
    pub deploy_min: f32,
    pub crew_min: u16,
    pub crew_optimal: u16,
    pub damage: [f32; 6],
    pub primary_damage_type: DamageType,
    pub armor_pen_mm: f32,
    pub frag_radius_m: f32,
    pub detonation_state: String,
    pub is_dud: bool,
    pub ignore_trench_cover: f32,
}

impl ResolvedAttackProfile {
    /// Accuracy decays smoothly with distance without any artificial maximum range cutoff.
    pub fn effective_accuracy_at(&self, dist_m: f32) -> f32 {
        let d = dist_m.max(0.0);
        (self.base_accuracy / (1.0 + self.acc_decay_coeff.max(0.0) * d)).clamp(0.0, 1.0)
    }

    /// Damage scales smoothly with distance without any artificial maximum range cutoff.
    pub fn effective_damage_at(&self, dist_m: f32) -> [f32; 6] {
        let d = dist_m.max(0.0);
        let mult = 1.0 / (1.0 + self.dmg_decay_coeff.max(0.0) * d);
        [
            self.damage[0] * mult,
            self.damage[1] * mult,
            self.damage[2] * mult,
            self.damage[3] * mult,
            self.damage[4] * mult,
            self.damage[5] * mult,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct ItemCatalog {
    pub version: String,
    pub schema: String,
    pub source: String,
    pub items: Vec<CompiledItem>,
    index_by_id: FxHashMap<String, usize>,
}

impl ItemCatalog {
    pub fn embedded() -> Result<Self, Vec<String>> {
        Self::load_from_str(EMBEDDED_ITEMS_CATALOG_JSON, "embedded://data/items_catalog.json")
    }

    pub fn load_from_file(path: &Path) -> Result<Self, Vec<String>> {
        let raw = std::fs::read_to_string(path).map_err(|e| {
            vec![format!(
                "failed to read catalog file {}: {}",
                path.display(),
                e
            )]
        })?;
        let cleaned = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
        Self::load_from_str(cleaned, &path.display().to_string())
    }

    pub fn default_search_paths() -> Vec<PathBuf> {
        vec![
            PathBuf::from("data/items_catalog.json"),
            PathBuf::from("../../data/items_catalog.json"),
            PathBuf::from(r"D:\tools\gentactix\data\items_catalog.json"),
        ]
    }

    pub fn load_preferred(explicit_path: Option<&Path>) -> Result<Self, Vec<String>> {
        if let Some(p) = explicit_path {
            return Self::load_from_file(p);
        }
        for candidate in Self::default_search_paths() {
            if candidate.is_file() {
                return Self::load_from_file(&candidate);
            }
        }
        Self::embedded()
    }

    pub fn load_from_str(json_str: &str, source: &str) -> Result<Self, Vec<String>> {
        let parsed: CatalogFile = serde_json::from_str(json_str)
            .map_err(|e| vec![format!("JSON parse error in {}: {}", source, e)])?;

        let mut errors = Vec::new();
        let mut index_by_id = FxHashMap::default();
        let mut items = Vec::with_capacity(parsed.items.len());

        for (idx, raw) in parsed.items.into_iter().enumerate() {
            if raw.id.trim().is_empty() {
                errors.push(format!("item at index {} has empty id", idx));
                continue;
            }
            if index_by_id.contains_key(&raw.id) {
                errors.push(format!("duplicate item id '{}'", raw.id));
                continue;
            }
            if raw.attrs.is_empty() {
                errors.push(format!("item '{}' has empty attrs array", raw.id));
            }
            if raw.attrs.iter().any(|a| a.key == "range_m") {
                errors.push(format!(
                    "FORBIDDEN attribute 'range_m' found on item '{}' (weapons must never have range limits)",
                    raw.id
                ));
            }

            index_by_id.insert(raw.id.clone(), items.len());
            items.push(compile_raw_item(raw, &mut errors));
        }

        validate_foreign_keys(&items, &index_by_id, &mut errors);

        if !errors.is_empty() {
            return Err(errors);
        }

        Ok(Self {
            version: parsed.version,
            schema: parsed.schema,
            source: source.to_string(),
            items,
            index_by_id,
        })
    }

    /// O(1) hash lookup by item id.
    pub fn get(&self, id: &str) -> Option<&CompiledItem> {
        self.index_by_id.get(id).map(|&idx| &self.items[idx])
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn to_raw_items(&self) -> Vec<CatalogItemRaw> {
        self.items
            .iter()
            .map(|c| CatalogItemRaw {
                id: c.id.clone(),
                name: c.name.clone(),
                kind: c.kind.clone(),
                attrs: c.attrs.clone(),
            })
            .collect()
    }

    /// Resolves effective combat profile from `(weapon_id, ammo_id, optional fuze_id, mount_ids)`.
    /// Enforces weapon -> ammo compatibility, mount modifiers, and fuze detonation mechanics.
    pub fn resolve_attack_profile(
        &self,
        weapon_id: &str,
        ammo_id: &str,
        fuze_id: Option<&str>,
        mount_ids: &[&str],
    ) -> Result<ResolvedAttackProfile, String> {
        let wpn_item = self
            .get(weapon_id)
            .ok_or_else(|| format!("unknown weapon_id '{}'", weapon_id))?;
        let wpn = wpn_item
            .weapon
            .as_ref()
            .ok_or_else(|| format!("item '{}' is not a weapon/launcher", weapon_id))?;

        let ammo_item = self
            .get(ammo_id)
            .ok_or_else(|| format!("unknown ammo_id '{}'", ammo_id))?;
        let ammo = ammo_item
            .ammo
            .as_ref()
            .ok_or_else(|| format!("item '{}' is not ammunition", ammo_id))?;

        if !wpn.compatible_ammo.iter().any(|id| id == ammo_id) {
            return Err(format!(
                "weapon '{}' cannot fire incompatible ammo '{}'",
                weapon_id, ammo_id
            ));
        }

        let mut combat_rpm = wpn.combat_rpm;
        let mut base_accuracy = wpn.base_accuracy;
        let mut acc_decay_coeff = wpn.acc_decay_coeff;
        let mut is_heavy_weapon = wpn.is_heavy_weapon;
        let mut deployable = wpn.deployable;
        let mut deploy_min = wpn.deploy_min;
        let mut crew_min = wpn.crew_min;
        let mut crew_optimal = wpn.crew_optimal;

        if !wpn.compatible_mounts.is_empty() && mount_ids.is_empty() {
            // Unmounted GPMG fired from hip/unbraced incurs accuracy penalty while remaining light.
            base_accuracy *= 0.60;
        }

        for mid in mount_ids {
            let m_item = self
                .get(mid)
                .ok_or_else(|| format!("unknown mount_id '{}'", mid))?;
            let m = m_item
                .mount
                .as_ref()
                .ok_or_else(|| format!("item '{}' is not a weapon mount", mid))?;
            if !m.compatible_weapons.iter().any(|id| id == weapon_id)
                && !wpn.compatible_mounts.iter().any(|id| id == mid)
            {
                return Err(format!(
                    "mount '{}' is incompatible with weapon '{}'",
                    mid, weapon_id
                ));
            }

            let mm = &m.modifiers;
            if let Some(v) = mm.is_heavy_weapon {
                is_heavy_weapon = v;
            }
            if let Some(v) = mm.deployable {
                deployable = v;
            }
            if let Some(v) = mm.deploy_min {
                deploy_min = v;
            }
            if let Some(v) = mm.combat_rpm_override {
                combat_rpm = v;
            }
            if let Some(v) = mm.base_accuracy_override {
                base_accuracy = v;
            } else if let Some(mult) = mm.base_accuracy_mult {
                base_accuracy *= mult;
            }
            if let Some(v) = mm.acc_decay_coeff_override {
                acc_decay_coeff = v;
            } else if let Some(mult) = mm.acc_decay_mult {
                acc_decay_coeff *= mult;
            }
            if let Some(v) = mm.crew_min {
                crew_min = v;
            }
            if let Some(v) = mm.crew_optimal {
                crew_optimal = v;
            }
        }

        let mut damage = ammo.damage;
        let mut frag_radius_m = ammo.frag_radius_m;
        let mut ignore_trench_cover = 0.0_f32;
        let mut is_dud = false;
        let detonation_state: String;

        if ammo.requires_fuze {
            match fuze_id {
                None => {
                    let behaves_as_solid_ap = ammo.no_fuze_behavior.contains("实心AP")
                        || ammo.no_fuze_behavior.contains("AP");
                    if behaves_as_solid_ap {
                        // Degrades to solid shot AP: loses explosive filler / -30%+ internal spall
                        damage[0] *= 0.35;
                        damage[1] *= 0.50;
                        damage[3] *= 0.70;
                        damage[4] *= 0.40;
                        damage[5] = 0.0;
                        frag_radius_m = 0.0;
                        detonation_state = "unfused_solid_ap".to_string();
                    } else {
                        // HE / HEAT / smoke / rocket without required fuze is a complete dud
                        damage = [0.0; 6];
                        frag_radius_m = 0.0;
                        is_dud = true;
                        detonation_state = "dud".to_string();
                    }
                }
                Some(fid) => {
                    let f_item = self
                        .get(fid)
                        .ok_or_else(|| format!("unknown fuze_id '{}'", fid))?;
                    let f = f_item
                        .fuze
                        .as_ref()
                        .ok_or_else(|| format!("item '{}' is not a fuze", fid))?;
                    if !ammo.compatible_fuzes.is_empty()
                        && !ammo.compatible_fuzes.iter().any(|id| id == fid)
                    {
                        return Err(format!(
                            "fuze '{}' is incompatible with ammo '{}'",
                            fid, ammo_id
                        ));
                    }
                    let em = &f.effect_mod;
                    if let Some(m) = em.anti_personnel_mult {
                        damage[0] *= m;
                    }
                    if let Some(m) = em.suppression_mult {
                        damage[1] *= m;
                    }
                    if let Some(m) = em.anti_soft_mult {
                        damage[2] *= m;
                    }
                    if let Some(m) = em.anti_armor_mult {
                        damage[3] *= m;
                    }
                    if let Some(m) = em.destructive_mult {
                        damage[4] *= m;
                    }
                    if let Some(m) = em.incendiary_mult {
                        damage[5] *= m;
                    }
                    if let Some(m) = em.frag_radius_mult {
                        frag_radius_m *= m;
                    }
                    if let Some(cov) = em.ignore_trench_cover {
                        ignore_trench_cover = cov.clamp(0.0, 1.0);
                    }
                    detonation_state = f.fuze_mode.clone();
                }
            }
        } else {
            if let Some(fid) = fuze_id {
                return Err(format!(
                    "ammo '{}' does not accept fuzes, got '{}'",
                    ammo_id, fid
                ));
            }
            detonation_state = "no_fuze_required".to_string();
        }

        let primary_damage_type = dominant_damage_type(&damage);

        Ok(ResolvedAttackProfile {
            weapon_id: weapon_id.to_string(),
            ammo_id: ammo_id.to_string(),
            fuze_id: fuze_id.map(|s| s.to_string()),
            mount_ids: mount_ids.iter().map(|s| (*s).to_string()).collect(),
            base_rpm: wpn.base_rpm,
            combat_rpm,
            base_accuracy: base_accuracy.clamp(0.0, 1.0),
            acc_decay_coeff,
            dmg_decay_coeff: wpn.dmg_decay_coeff,
            is_heavy_weapon,
            deployable,
            deploy_min,
            crew_min,
            crew_optimal,
            damage,
            primary_damage_type,
            armor_pen_mm: ammo.armor_pen_mm,
            frag_radius_m,
            detonation_state,
            is_dud,
            ignore_trench_cover,
        })
    }
}

fn dominant_damage_type(dmg: &[f32; 6]) -> DamageType {
    let mut best_idx = 0usize;
    let mut best_val = dmg[0];
    for (i, &val) in dmg.iter().enumerate().skip(1) {
        if val > best_val {
            best_val = val;
            best_idx = i;
        }
    }
    match best_idx {
        0 => DamageType::AntiPersonnel,
        1 => DamageType::Suppression,
        2 => DamageType::AntiSoft,
        3 => DamageType::AntiArmor,
        4 => DamageType::Destructive,
        5 => DamageType::Incendiary,
        _ => DamageType::AntiPersonnel,
    }
}

fn compile_raw_item(raw: CatalogItemRaw, errors: &mut Vec<String>) -> CompiledItem {
    let mut tmp = CompiledItem {
        id: raw.id,
        name: raw.name,
        kind: raw.kind,
        category: String::new(),
        weight_kg: 0.0,
        grid_w: 1,
        grid_h: 1,
        stack_max: 1,
        can_hand: true,
        attrs: raw.attrs,
        weapon: None,
        ammo: None,
        fuze: None,
        mount: None,
    };

    tmp.category = tmp.attr_str("category").unwrap_or("").to_string();
    tmp.weight_kg = tmp
        .attr_f32("weight_kg")
        .or_else(|| tmp.attr_f32("weight_kg_per_round"))
        .unwrap_or(0.0);
    tmp.grid_w = tmp.attr_u16("grid_w").unwrap_or(1).max(1);
    tmp.grid_h = tmp.attr_u16("grid_h").unwrap_or(1).max(1);
    tmp.stack_max = tmp.attr_u16("stack_max").unwrap_or(1).max(1);
    tmp.can_hand = tmp.attr_bool("can_hand").unwrap_or(true);

    let is_weapon_kind = matches!(
        tmp.kind.as_str(),
        "weapon" | "weapon_attachment" | "weapon_vehicle_placeholder"
    );
    if is_weapon_kind {
        let base_rpm = tmp.attr_f32("base_rpm").unwrap_or(0.0);
        let combat_rpm = tmp.attr_f32("combat_rpm").unwrap_or(base_rpm);
        let base_accuracy = tmp.attr_f32("base_accuracy").unwrap_or(0.5);
        if base_rpm <= 0.0 {
            errors.push(format!("weapon '{}' missing positive base_rpm", tmp.id));
        }
        if !(0.0..=1.0).contains(&base_accuracy) {
            errors.push(format!(
                "weapon '{}' has invalid base_accuracy {}",
                tmp.id, base_accuracy
            ));
        }
        let crew_min = tmp.attr_u16("crew_min").unwrap_or(1).max(1);
        let crew_optimal = tmp.attr_u16("crew_optimal").unwrap_or(crew_min);
        tmp.weapon = Some(CompiledWeapon {
            caliber: tmp.attr_str("caliber").unwrap_or("").to_string(),
            compatible_ammo: tmp.attr_str_list("compatible_ammo"),
            compatible_mounts: tmp.attr_str_list("compatible_mounts"),
            feed_type: tmp.attr_str("feed_type").unwrap_or("").to_string(),
            base_rpm,
            combat_rpm,
            base_accuracy,
            acc_decay_coeff: tmp.attr_f32("acc_decay_coeff").unwrap_or(0.0),
            dmg_decay_coeff: tmp.attr_f32("dmg_decay_coeff").unwrap_or(0.0),
            is_heavy_weapon: tmp.attr_bool("is_heavy_weapon").unwrap_or(false),
            crew_min,
            crew_optimal,
            indirect: tmp.attr_bool("indirect").unwrap_or(false),
            direct_capable: tmp.attr_bool("direct_capable").unwrap_or(true),
            deployable: tmp.attr_bool("deployable").unwrap_or(false),
            deploy_min: tmp.attr_f32("deploy_min").unwrap_or(0.0),
            towable: tmp.attr_bool("towable").unwrap_or(false),
            is_placeholder: tmp.attr_bool("is_placeholder").unwrap_or(false),
        });
    } else if matches!(tmp.kind.as_str(), "ammo" | "mine" | "explosive") {
        tmp.ammo = Some(CompiledAmmo {
            caliber: tmp.attr_str("caliber").unwrap_or("").to_string(),
            damage: [
                tmp.attr_f32("anti_personnel").unwrap_or(0.0),
                tmp.attr_f32("suppression").unwrap_or(0.0),
                tmp.attr_f32("anti_soft").unwrap_or(0.0),
                tmp.attr_f32("anti_armor").unwrap_or(0.0),
                tmp.attr_f32("destructive").unwrap_or(0.0),
                tmp.attr_f32("incendiary").unwrap_or(0.0),
            ],
            armor_pen_mm: tmp.attr_f32("armor_pen_mm").unwrap_or(0.0),
            frag_radius_m: tmp.attr_f32("frag_radius_m").unwrap_or(0.0),
            requires_fuze: tmp.attr_bool("requires_fuze").unwrap_or(false),
            compatible_fuzes: tmp.attr_str_list("compatible_fuzes"),
            no_fuze_behavior: tmp.attr_str("no_fuze_behavior").unwrap_or("哑弹").to_string(),
        });
    } else if tmp.kind == "fuze" {
        let effect_mod = match tmp.attr("effect_mod") {
            Some(v) => serde_json::from_value::<FuzeEffectMod>(v.clone()).unwrap_or_else(|e| {
                errors.push(format!("fuze '{}' invalid effect_mod: {}", tmp.id, e));
                FuzeEffectMod::default()
            }),
            None => FuzeEffectMod::default(),
        };
        tmp.fuze = Some(CompiledFuze {
            fuze_mode: tmp.attr_str("fuze_mode").unwrap_or("instant").to_string(),
            delay_s: tmp.attr_f32("delay_s").unwrap_or(0.0),
            timer_max_s: tmp.attr_f32("timer_max_s").unwrap_or(0.0),
            compatible_calibers: tmp.attr_str_list("compatible_calibers"),
            effect_mod,
        });
    } else if tmp.kind == "mount" {
        let modifiers = match tmp.attr("mount_modifiers") {
            Some(v) => serde_json::from_value::<MountModifiers>(v.clone()).unwrap_or_else(|e| {
                errors.push(format!("mount '{}' invalid mount_modifiers: {}", tmp.id, e));
                MountModifiers::default()
            }),
            None => MountModifiers::default(),
        };
        tmp.mount = Some(CompiledMount {
            compatible_weapons: tmp.attr_str_list("compatible_weapons"),
            modifiers,
        });
    }

    tmp
}

fn validate_foreign_keys(
    items: &[CompiledItem],
    index: &FxHashMap<String, usize>,
    errors: &mut Vec<String>,
) {
    let mut seen_keys = FxHashSet::default();
    for item in items {
        seen_keys.clear();
        for attr in &item.attrs {
            if !seen_keys.insert(attr.key.as_str()) {
                errors.push(format!("duplicate attr key '{}' on '{}'", attr.key, item.id));
            }
        }
        for fk_list in [
            "compatible_ammo",
            "compatible_fuzes",
            "compatible_mounts",
            "compatible_weapons",
        ] {
            for target in item.attr_str_list(fk_list) {
                if !index.contains_key(&target) {
                    errors.push(format!(
                        "dangling reference: {}.{} -> '{}'",
                        item.id, fk_list, target
                    ));
                }
            }
        }
        for fk_scalar in ["requires_battery", "requires_host"] {
            if let Some(target) = item.attr_str(fk_scalar) {
                if !index.contains_key(target) {
                    errors.push(format!(
                        "dangling reference: {}.{} -> '{}'",
                        item.id, fk_scalar, target
                    ));
                }
            }
        }
    }
}

static GLOBAL_CATALOG: OnceLock<RwLock<Arc<ItemCatalog>>> = OnceLock::new();

pub fn global_catalog() -> Arc<ItemCatalog> {
    let lock = GLOBAL_CATALOG.get_or_init(|| {
        let loaded = ItemCatalog::load_preferred(None).unwrap_or_else(|errs| {
            panic!("failed to load items catalog: {}", errs.join("; "));
        });
        RwLock::new(Arc::new(loaded))
    });
    Arc::clone(&lock.read().unwrap())
}

pub fn reload_global_catalog(explicit_path: Option<&Path>) -> Result<Arc<ItemCatalog>, Vec<String>> {
    let loaded = ItemCatalog::load_preferred(explicit_path)?;
    let arc = Arc::new(loaded);
    let lock = GLOBAL_CATALOG.get_or_init(|| RwLock::new(Arc::clone(&arc)));
    *lock.write().unwrap() = Arc::clone(&arc);
    Ok(arc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_loads_cleanly_with_no_range_limits() {
        let cat = ItemCatalog::embedded().expect("embedded catalog must parse and validate");
        assert!(cat.len() >= 250, "expected >= 250 items, got {}", cat.len());
        for item in &cat.items {
            assert!(!item.attrs.is_empty(), "{} must have attrs[]", item.id);
            assert!(
                item.attr("range_m").is_none(),
                "item {} must not contain range_m",
                item.id
            );
        }
        // O(1) lookup checks across categories
        assert!(cat.get("wpn_flak41_88").is_some());
        assert!(cat.get("wpn_flak36_88").is_some());
        assert!(cat.get("wpn_flak38_105").is_some());
        assert!(cat.get("wpn_flak40_128").is_some());
        assert!(cat.get("wpn_pak43_cruciform").is_some());
        assert!(cat.get("wpn_pak43_41").is_some());
        assert!(cat.get("fuze_ztz_s30_airburst").is_some());
        assert!(cat.get("tool_fuze_wrench").is_some());
        assert!(cat.get("gear_stahlhelm_m35").is_some());
    }

    #[test]
    fn validator_rejects_range_m_and_dangling_foreign_key() {
        let bad_json = r#"{
            "version": "1.0",
            "schema": "gentactix.items_catalog.v1",
            "items": [
                {
                    "id": "wpn_bad",
                    "name": "Bad Gun",
                    "kind": "weapon",
                    "attrs": [
                        { "key": "base_rpm", "label": "rpm", "val": 10 },
                        { "key": "base_accuracy", "label": "acc", "val": 0.8 },
                        { "key": "range_m", "label": "illegal", "val": 450 },
                        { "key": "compatible_ammo", "label": "ammo", "val": ["missing_ammo"] }
                    ]
                }
            ]
        }"#;
        let errs = ItemCatalog::load_from_str(bad_json, "test").unwrap_err();
        assert!(errs.iter().any(|e| e.contains("range_m")));
        assert!(errs.iter().any(|e| e.contains("missing_ammo")));
    }

    #[test]
    fn mg42_mount_swapping_toggles_heavy_weapon_and_rpm() {
        let cat = ItemCatalog::embedded().unwrap();

        let bare = cat
            .resolve_attack_profile("wpn_mg42", "ammo_792x57_ss", None, &[])
            .unwrap();
        assert!(!bare.is_heavy_weapon);
        assert!((bare.base_accuracy - 0.74 * 0.60).abs() < 1e-4);

        let bipod = cat
            .resolve_attack_profile("wpn_mg42", "ammo_792x57_ss", None, &["mount_zweibein_34_42"])
            .unwrap();
        assert!(!bipod.is_heavy_weapon);
        assert!(!bipod.deployable);
        assert_eq!(bipod.combat_rpm, 350.0);
        assert!((bipod.base_accuracy - 0.74).abs() < 1e-4);

        let lafette = cat
            .resolve_attack_profile("wpn_mg42", "ammo_792x57_pmk", None, &["mount_lafette_42"])
            .unwrap();
        assert!(lafette.is_heavy_weapon);
        assert!(lafette.deployable);
        assert_eq!(lafette.combat_rpm, 550.0);
        assert!((lafette.base_accuracy - 0.88).abs() < 1e-4);
        assert!(lafette.damage[5] > 0.8, "P.m.K. must deliver incendiary damage");
        assert!(lafette.effective_accuracy_at(2500.0) > 0.0, "no hard distance cutoff");
    }

    #[test]
    fn artillery_fuzes_switch_between_dud_impact_delay_and_airburst() {
        let cat = ItemCatalog::embedded().unwrap();

        let dud_round = cat
            .resolve_attack_profile("wpn_lefh18", "ammo_105_fhgr_38", None, &[])
            .unwrap();
        assert!(dud_round.is_dud);
        assert_eq!(dud_round.detonation_state, "dud");
        assert_eq!(dud_round.damage, [0.0; 6]);

        let delay_round = cat
            .resolve_attack_profile(
                "wpn_lefh18",
                "ammo_105_fhgr_38",
                Some("fuze_az23_v_delay"),
                &[],
            )
            .unwrap();
        assert!(!delay_round.is_dud);
        assert!(delay_round.damage[4] > 50.0, "delay fuze boosts destructive power");
        assert!(delay_round.damage[0] < 52.0, "delay fuze reduces surface anti-personnel");

        let beton_round = cat
            .resolve_attack_profile(
                "wpn_sfh18_15cm",
                "ammo_150sfh_gr19_beton",
                Some("fuze_az23_v_delay"),
                &[],
            )
            .unwrap();
        assert_eq!(beton_round.primary_damage_type, DamageType::Destructive);

        let airburst_round = cat
            .resolve_attack_profile(
                "wpn_flak36_88",
                "ammo_88l56_sprgr_l45",
                Some("fuze_ztz_s30_airburst"),
                &[],
            )
            .unwrap();
        assert!(!airburst_round.is_dud);
        assert!(airburst_round.damage[0] > 50.0);
        assert!(airburst_round.ignore_trench_cover >= 0.59);
        assert!(airburst_round.frag_radius_m > 45.0);

        let unfused_ap = cat
            .resolve_attack_profile("wpn_flak36_88", "ammo_88l56_pzgr39", None, &[])
            .unwrap();
        assert!(!unfused_ap.is_dud);
        assert_eq!(unfused_ap.detonation_state, "unfused_solid_ap");
        assert!(unfused_ap.damage[3] > 40.0);
    }
}
