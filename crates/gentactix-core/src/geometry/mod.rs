//! `geometry` module: Doctrine 2^3 = 8m Canonical Tactical Grid Geometry
//! 遵循 GTX-CORE-SPEC-FINAL-2.0 规格书与 plan.txt Phase 01 要求：
//! 1. 唯一 Doctrine 格边长 8.0m (2^3 m, shift 3)
//! 2. 强类型连续米制坐标与离散网格索引 (r, c) 严格隔离
//! 3. 严禁静默 clamp 或无符号越界回绕

use serde::{Deserialize, Serialize};

/// 核心战术网格基准尺度 (Doctrine 参数，不可由场景或实现覆盖)
pub const TACTICAL_CELL_SIZE_M: f32 = 8.0;
pub const TACTICAL_CELL_SHIFT: u32 = 3;
pub const TACTICAL_CELL_AREA_M2: f32 = TACTICAL_CELL_SIZE_M * TACTICAL_CELL_SIZE_M; // 64.0 m^2

/// 宏观网格层级尺度 (2^8 m = 256m)
pub const MACRO_CELL_SIZE_M: f32 = 256.0;
pub const MACRO_CELL_SHIFT: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GeometryId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize, Default)]
pub struct Meter(pub f32);

impl Meter {
    #[inline]
    pub fn value(self) -> f32 {
        self.0
    }
}

impl std::ops::Add for Meter {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Meter(self.0 + rhs.0)
    }
}

impl std::ops::Sub for Meter {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Meter(self.0 - rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CellCoord {
    pub r: i32,
    pub c: i32,
}

impl CellCoord {
    #[inline]
    pub const fn new(r: i32, c: i32) -> Self {
        Self { r, c }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorldPos {
    pub x: f32, // meters along c (world x)
    pub y: f32, // meters along r (world y)
}

impl WorldPos {
    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[inline]
    pub fn distance_to(self, other: WorldPos) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx.hypot(dy)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OutsideBoundsError {
    NegativeCoordinate { x: f32, y: f32 },
    OutsideWorldExtent { x: f32, y: f32, extent_x: f32, extent_y: f32 },
    InvalidCellIndex { r: i32, c: i32, rows: u32, cols: u32 },
}

impl std::fmt::Display for OutsideBoundsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NegativeCoordinate { x, y } => {
                write!(f, "Coordinate ({x}, {y}) is negative before origin offset")
            }
            Self::OutsideWorldExtent { x, y, extent_x, extent_y } => {
                write!(f, "Coordinate ({x}, {y}) exceeds world extent ({extent_x}, {extent_y})")
            }
            Self::InvalidCellIndex { r, c, rows, cols } => {
                write!(f, "Cell index ({r}, {c}) outside bounds [0..{rows}, 0..{cols})")
            }
        }
    }
}

impl std::error::Error for OutsideBoundsError {}

/// 核心战术网格几何体 (GridGeometry)
/// 格边长严格等于 2^3 = 8.0 米，由 Doctrine 提供，场景只能决定 rows, cols 与 origin_m
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridGeometry {
    pub id: GeometryId,
    pub origin_m: [f32; 2], // [x, y] in meters
    pub rows: u32,
    pub cols: u32,
}

impl GridGeometry {
    /// 标准构造器
    pub fn new(id: GeometryId, origin_m: [f32; 2], rows: u32, cols: u32) -> Result<Self, &'static str> {
        if !origin_m[0].is_finite() || !origin_m[1].is_finite() {
            return Err("Origin must be finite");
        }
        if rows == 0 || cols == 0 {
            return Err("Grid rows and cols must be non-zero");
        }
        Ok(Self {
            id,
            origin_m,
            rows,
            cols,
        })
    }

    /// 标准 1024x1024 战术推演几何 (8192m x 8192m)
    pub fn standard_1024(id: u32, origin_m: [f32; 2]) -> Self {
        Self {
            id: GeometryId(id),
            origin_m,
            rows: 1024,
            cols: 1024,
        }
    }

    /// 单元物理格边长 (米)
    #[inline]
    pub const fn cell_size_m(&self) -> f32 {
        TACTICAL_CELL_SIZE_M
    }

    /// 单元物理面积 (平方米)
    #[inline]
    pub const fn cell_area_m2(&self) -> f32 {
        TACTICAL_CELL_AREA_M2
    }

    /// 世界总宽度 (x / cols) 米制范围
    #[inline]
    pub fn world_width_m(&self) -> f32 {
        self.cols as f32 * TACTICAL_CELL_SIZE_M
    }

    /// 世界总高度 (y / rows) 米制范围
    #[inline]
    pub fn world_height_m(&self) -> f32 {
        self.rows as f32 * TACTICAL_CELL_SIZE_M
    }

    /// 世界覆盖总跨度 [width_m, height_m]
    #[inline]
    pub fn world_extent_m(&self) -> [f32; 2] {
        [self.world_width_m(), self.world_height_m()]
    }

    /// 检查 (r, c) 是否在网格索引界内
    #[inline]
    pub fn in_bounds(&self, r: i32, c: i32) -> bool {
        r >= 0 && r < self.rows as i32 && c >= 0 && c < self.cols as i32
    }

    /// 连续世界坐标转换为网格离散索引 (r, c)
    /// 严禁静默 clamp，越界必须明确报错
    pub fn world_to_cell(&self, pos: WorldPos) -> Result<CellCoord, OutsideBoundsError> {
        let rel_x = pos.x - self.origin_m[0];
        let rel_y = pos.y - self.origin_m[1];

        if rel_x < 0.0 || rel_y < 0.0 {
            return Err(OutsideBoundsError::NegativeCoordinate { x: pos.x, y: pos.y });
        }

        let width = self.world_width_m();
        let height = self.world_height_m();
        if rel_x >= width || rel_y >= height {
            return Err(OutsideBoundsError::OutsideWorldExtent {
                x: pos.x,
                y: pos.y,
                extent_x: self.origin_m[0] + width,
                extent_y: self.origin_m[1] + height,
            });
        }

        // c = floor((x - origin_x) / 8m)
        // r = floor((y - origin_y) / 8m)
        let c = (rel_x * (1.0 / TACTICAL_CELL_SIZE_M)).floor() as i32;
        let r = (rel_y * (1.0 / TACTICAL_CELL_SIZE_M)).floor() as i32;

        if !self.in_bounds(r, c) {
            return Err(OutsideBoundsError::InvalidCellIndex {
                r,
                c,
                rows: self.rows,
                cols: self.cols,
            });
        }

        Ok(CellCoord { r, c })
    }

    /// 二进制定点坐标 (含 F 位小数) 转网格索引
    /// 使用算术右移，处理有符号数，保证负坐标不溢出为巨大正数
    pub fn fixed_world_to_cell(&self, fixed_x: i64, fixed_y: i64, frac_bits: u32) -> Result<CellCoord, OutsideBoundsError> {
        let shift = TACTICAL_CELL_SHIFT + frac_bits;
        let c_i64 = fixed_x >> shift;
        let r_i64 = fixed_y >> shift;

        if c_i64 < 0 || r_i64 < 0 {
            return Err(OutsideBoundsError::NegativeCoordinate {
                x: (fixed_x as f32) / (1 << frac_bits) as f32,
                y: (fixed_y as f32) / (1 << frac_bits) as f32,
            });
        }

        if c_i64 >= self.cols as i64 || r_i64 >= self.rows as i64 {
            return Err(OutsideBoundsError::InvalidCellIndex {
                r: r_i64 as i32,
                c: c_i64 as i32,
                rows: self.rows,
                cols: self.cols,
            });
        }

        Ok(CellCoord {
            r: r_i64 as i32,
            c: c_i64 as i32,
        })
    }

    /// 单元中心连续世界坐标 (米)
    pub fn cell_center_m(&self, coord: CellCoord) -> Result<WorldPos, OutsideBoundsError> {
        if !self.in_bounds(coord.r, coord.c) {
            return Err(OutsideBoundsError::InvalidCellIndex {
                r: coord.r,
                c: coord.c,
                rows: self.rows,
                cols: self.cols,
            });
        }
        let cx = self.origin_m[0] + (coord.c as f32 + 0.5) * TACTICAL_CELL_SIZE_M;
        let cy = self.origin_m[1] + (coord.r as f32 + 0.5) * TACTICAL_CELL_SIZE_M;
        Ok(WorldPos::new(cx, cy))
    }

    /// 单元世界边界框 [[min_x, min_y], [max_x, max_y]]
    pub fn cell_bounds_m(&self, coord: CellCoord) -> Result<([f32; 2], [f32; 2]), OutsideBoundsError> {
        if !self.in_bounds(coord.r, coord.c) {
            return Err(OutsideBoundsError::InvalidCellIndex {
                r: coord.r,
                c: coord.c,
                rows: self.rows,
                cols: self.cols,
            });
        }
        let min_x = self.origin_m[0] + coord.c as f32 * TACTICAL_CELL_SIZE_M;
        let min_y = self.origin_m[1] + coord.r as f32 * TACTICAL_CELL_SIZE_M;
        let max_x = min_x + TACTICAL_CELL_SIZE_M;
        let max_y = min_y + TACTICAL_CELL_SIZE_M;
        Ok(([min_x, min_y], [max_x, max_y]))
    }

    /// 将物理米制核半径 (kernel radius in meters) 转换为单元覆盖支撑跨度 (半宽 cell count)
    #[inline]
    pub fn kernel_radius_to_cell_support(&self, radius_m: f32) -> i32 {
        if radius_m <= 0.0 {
            return 0;
        }
        (radius_m * (1.0 / TACTICAL_CELL_SIZE_M)).ceil() as i32
    }

    /// 计算两连续位置的欧式米制距离
    #[inline]
    pub fn distance_m(&self, p1: WorldPos, p2: WorldPos) -> f32 {
        p1.distance_to(p2)
    }

    /// 计算两网格单元中心的欧式米制距离
    pub fn cell_distance_m(&self, c1: CellCoord, c2: CellCoord) -> Result<f32, OutsideBoundsError> {
        let p1 = self.cell_center_m(c1)?;
        let p2 = self.cell_center_m(c2)?;
        Ok(p1.distance_to(p2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_01_standard_1024_extent_is_8192m() {
        let geom = GridGeometry::standard_1024(1, [0.0, 0.0]);
        assert_eq!(geom.rows, 1024);
        assert_eq!(geom.cols, 1024);
        assert_eq!(geom.cell_size_m(), 8.0);
        assert_eq!(geom.cell_area_m2(), 64.0);

        let [w, h] = geom.world_extent_m();
        assert_eq!(w, 8192.0, "1024 cells @ 8m must equal 8192.0m width");
        assert_eq!(h, 8192.0, "1024 cells @ 8m must equal 8192.0m height");
    }

    #[test]
    fn test_phase_01_coordinate_to_cell_rounding_and_centers() {
        let geom = GridGeometry::standard_1024(1, [100.0, 200.0]);

        // 位于单元内 (c=0, r=0)
        let pos1 = WorldPos::new(104.0, 204.0);
        let cell1 = geom.world_to_cell(pos1).unwrap();
        assert_eq!(cell1, CellCoord::new(0, 0));

        let center1 = geom.cell_center_m(cell1).unwrap();
        assert_eq!(center1, WorldPos::new(104.0, 204.0));

        // 边界点 (108.0, 208.0) 应当进入 (c=1, r=1)
        let pos_edge = WorldPos::new(108.0, 208.0);
        let cell_edge = geom.world_to_cell(pos_edge).unwrap();
        assert_eq!(cell_edge, CellCoord::new(1, 1));

        // 单元边界框测试
        let (min_b, max_b) = geom.cell_bounds_m(cell_edge).unwrap();
        assert_eq!(min_b, [108.0, 208.0]);
        assert_eq!(max_b, [116.0, 216.0]);
    }

    #[test]
    fn test_phase_01_bounds_and_negative_coordinates_no_wrapping() {
        let geom = GridGeometry::standard_1024(1, [0.0, 0.0]);

        // 负坐标绝不通过无符号强转变为巨大索引
        let neg_pos = WorldPos::new(-1.0, 10.0);
        assert!(matches!(
            geom.world_to_cell(neg_pos),
            Err(OutsideBoundsError::NegativeCoordinate { .. })
        ));

        // 超出世界边界
        let out_pos = WorldPos::new(8192.0, 100.0);
        assert!(matches!(
            geom.world_to_cell(out_pos),
            Err(OutsideBoundsError::OutsideWorldExtent { .. })
        ));

        // 二进制定点转换有符号测试 (frac_bits = 4, shift = 3 + 4 = 7)
        let fixed_neg = -16i64; // -1.0m
        assert!(geom.fixed_world_to_cell(fixed_neg, 128, 4).is_err());

        let fixed_valid = 128i64; // 8.0m -> cell c = 1
        let c_fixed = geom.fixed_world_to_cell(fixed_valid, 256, 4).unwrap();
        assert_eq!(c_fixed, CellCoord::new(2, 1));
    }

    #[test]
    fn test_phase_01_kernel_radius_support_conversion() {
        let geom = GridGeometry::standard_1024(1, [0.0, 0.0]);
        // 半径 0m -> 0格
        assert_eq!(geom.kernel_radius_to_cell_support(0.0), 0);
        // 半径 5m -> ceil(5 / 8) = 1格
        assert_eq!(geom.kernel_radius_to_cell_support(5.0), 1);
        // 半径 8m -> ceil(8 / 8) = 1格
        assert_eq!(geom.kernel_radius_to_cell_support(8.0), 1);
        // 半径 12m -> ceil(12 / 8) = 2格
        assert_eq!(geom.kernel_radius_to_cell_support(12.0), 2);
        // 半径 25m -> ceil(25 / 8) = 4格
        assert_eq!(geom.kernel_radius_to_cell_support(25.0), 4);
    }
}
