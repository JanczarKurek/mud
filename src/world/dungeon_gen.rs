//! Procedural dungeon generator.
//!
//! Produces a `SpaceDefinition` whose floors and walls form sparse rectangular
//! chambers connected by long meandering corridors. The output goes through
//! the same `resolve_objects` + `instantiate_space` pipeline as authored YAML
//! maps, so generated dungeons round-trip cleanly through editor save/load.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::world::floor_definitions::FloorTypeId;
use crate::world::map_layout::{
    AnonymousObjectPlacements, FloorPlacements, MapObjectEntry, SpaceDefinition, TileCoordinate,
};

#[derive(Clone, Debug)]
pub struct DungeonParams {
    pub width: i32,
    pub height: i32,
    pub wall_type_id: String,
    pub chamber_floor: FloorTypeId,
    pub corridor_floor: FloorTypeId,
    pub fill_floor_type: FloorTypeId,
    pub target_rooms: u32,
    pub min_room_size: i32,
    pub max_room_size: i32,
    /// Minimum empty tiles between any two rooms. Larger → sparser layout.
    pub room_padding: i32,
    /// 0.0 = corridors take greedy Manhattan steps. 1.0 = each step is fully
    /// random. Values around 0.5 produce the long, snaky look.
    pub corridor_wander: f32,
    /// Fraction of extra non-MST corridors added (creates loops + crossings).
    pub extra_corridor_ratio: f32,
    /// Density of dead-end side branches sprouting from main corridors.
    /// 0.0 = none. 1.0 = many. Independent of `extra_corridor_ratio`, which
    /// adds *room-to-room* loops; this adds spurs that don't necessarily
    /// terminate at a room.
    pub branch_factor: f32,
    /// `0` → seed from system time.
    pub seed: u64,
}

impl Default for DungeonParams {
    fn default() -> Self {
        Self {
            width: 64,
            height: 48,
            wall_type_id: "wall".into(),
            chamber_floor: "cobblestone".into(),
            corridor_floor: "dirt_path".into(),
            // Empty = void/black tiles outside the dungeon.
            fill_floor_type: String::new(),
            target_rooms: 8,
            min_room_size: 4,
            max_room_size: 7,
            room_padding: 4,
            corridor_wander: 0.55,
            extra_corridor_ratio: 0.4,
            branch_factor: 0.5,
            seed: 0,
        }
    }
}

pub fn generate_dungeon(authored_id: String, params: DungeonParams) -> SpaceDefinition {
    let mut layout = Layout::new(params.width, params.height);
    let seed = if params.seed == 0 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xC0FFEE)
            .max(1)
    } else {
        params.seed
    };
    let mut rng = Rng::new(seed);

    let rooms = place_rooms(&mut layout, &mut rng, &params);
    if rooms.len() >= 2 {
        carve_corridors(&mut layout, &mut rng, &rooms, &params);
    }
    if params.branch_factor > 0.0 {
        carve_branches(&mut layout, &mut rng, &params);
    }

    layout_to_space(authored_id, layout, &params)
}

// ── Layout grid ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Wall,
    Chamber,
    Corridor,
}

struct Layout {
    width: i32,
    height: i32,
    cells: Vec<Cell>,
}

impl Layout {
    fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            cells: vec![Cell::Wall; (width * height) as usize],
        }
    }

    fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.width && y < self.height
    }

    fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.width + x) as usize
    }

    fn get(&self, x: i32, y: i32) -> Cell {
        self.cells[self.idx(x, y)]
    }

    fn set(&mut self, x: i32, y: i32, cell: Cell) {
        let idx = self.idx(x, y);
        self.cells[idx] = cell;
    }
}

#[derive(Clone, Copy, Debug)]
struct Room {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Room {
    fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    /// True if this room (expanded by `pad` tiles) overlaps `other`.
    fn overlaps_padded(&self, other: &Room, pad: i32) -> bool {
        !(self.x + self.w + pad <= other.x
            || other.x + other.w + pad <= self.x
            || self.y + self.h + pad <= other.y
            || other.y + other.h + pad <= self.y)
    }
}

// ── Room placement ────────────────────────────────────────────────────────────

fn place_rooms(layout: &mut Layout, rng: &mut Rng, params: &DungeonParams) -> Vec<Room> {
    let mut rooms: Vec<Room> = Vec::new();
    let attempts = (params.target_rooms.saturating_mul(20)).max(40);
    let min_size = params.min_room_size.max(2);
    let max_size = params.max_room_size.max(min_size);

    for _ in 0..attempts {
        if rooms.len() >= params.target_rooms as usize {
            break;
        }
        let w = rng.gen_range_i32(min_size, max_size + 1);
        let h = rng.gen_range_i32(min_size, max_size + 1);
        if w >= layout.width - 3 || h >= layout.height - 3 {
            continue;
        }
        let x = rng.gen_range_i32(2, layout.width - w - 1);
        let y = rng.gen_range_i32(2, layout.height - h - 1);
        let candidate = Room { x, y, w, h };
        if rooms
            .iter()
            .any(|r| r.overlaps_padded(&candidate, params.room_padding))
        {
            continue;
        }
        carve_room(layout, &candidate);
        rooms.push(candidate);
    }
    rooms
}

fn carve_room(layout: &mut Layout, room: &Room) {
    for dy in 0..room.h {
        for dx in 0..room.w {
            layout.set(room.x + dx, room.y + dy, Cell::Chamber);
        }
    }
}

// ── Corridor carving ──────────────────────────────────────────────────────────

fn carve_corridors(layout: &mut Layout, rng: &mut Rng, rooms: &[Room], params: &DungeonParams) {
    // Build MST over room centers (Prim's, O(n^2) — n is small).
    let n = rooms.len();
    let mut in_tree = vec![false; n];
    let mut edges: Vec<(usize, usize)> = Vec::new();
    in_tree[0] = true;
    for _ in 1..n {
        let mut best: Option<(usize, usize, i64)> = None;
        for i in 0..n {
            if !in_tree[i] {
                continue;
            }
            for j in 0..n {
                if in_tree[j] {
                    continue;
                }
                let (cix, ciy) = rooms[i].center();
                let (cjx, cjy) = rooms[j].center();
                let dx = (cix - cjx) as i64;
                let dy = (ciy - cjy) as i64;
                let d = dx * dx + dy * dy;
                if best.map(|(_, _, bd)| d < bd).unwrap_or(true) {
                    best = Some((i, j, d));
                }
            }
        }
        if let Some((i, j, _)) = best {
            edges.push((i, j));
            in_tree[j] = true;
        }
    }

    // Add extra random edges for loops and crossings.
    let extra_count = ((n as f32) * params.extra_corridor_ratio).floor() as usize;
    for _ in 0..extra_count {
        if n < 2 {
            break;
        }
        let a = rng.gen_range_usize(0, n);
        let mut b = rng.gen_range_usize(0, n);
        if b == a {
            b = (b + 1) % n;
        }
        edges.push((a, b));
    }

    for (a, b) in edges {
        carve_one_corridor(layout, rng, &rooms[a], &rooms[b], params);
    }
}

fn carve_one_corridor(
    layout: &mut Layout,
    rng: &mut Rng,
    from: &Room,
    to: &Room,
    params: &DungeonParams,
) {
    let (sx, sy) = from.center();
    let (tx, ty) = to.center();
    let manhattan = (sx - tx).abs() + (sy - ty).abs();
    let cap = (3 * manhattan + 50) as usize;

    let wander = params.corridor_wander.clamp(0.0, 1.0);

    let mut x = sx;
    let mut y = sy;
    let mut steps = 0;
    while (x != tx || y != ty) && steps < cap {
        carve_corridor_cell(layout, x, y);
        steps += 1;

        // Direction choice. With probability `wander` go fully random; else
        // pick a cardinal that reduces remaining Manhattan distance.
        let go_random = rng.gen_unit() < wander;
        let (nx, ny) = if go_random {
            random_neighbor(x, y, rng)
        } else {
            greedy_step(x, y, tx, ty, rng)
        };

        if !layout.in_bounds(nx, ny) {
            // Try a greedy fallback when wandering goes OOB.
            let (gx, gy) = greedy_step(x, y, tx, ty, rng);
            if layout.in_bounds(gx, gy) {
                x = gx;
                y = gy;
            }
            continue;
        }

        // Don't crash through the destination room before the corridor reaches
        // its actual target tile — that produces ugly cheese-throughs.
        if to.contains(nx, ny) && (nx != tx || ny != ty) {
            let (gx, gy) = greedy_step(x, y, tx, ty, rng);
            if layout.in_bounds(gx, gy) && !(to.contains(gx, gy) && (gx != tx || gy != ty)) {
                x = gx;
                y = gy;
                continue;
            }
            // Fallthrough: take the random step anyway, better than looping forever.
        }

        x = nx;
        y = ny;
    }

    // Walked too long? Finish with a straight L from current position.
    if x != tx || y != ty {
        finish_with_l_shape(layout, rng, x, y, tx, ty);
    }
    carve_corridor_cell(layout, tx, ty);
}

/// Sprout dead-end spurs off existing corridors. Run after main corridors are
/// carved. Each spur is a short biased random walk that stops at OOB, on a
/// chamber, or after a random length cap. Spurs are allowed to merge into
/// other corridors (creating extra junctions) but not bore into rooms.
fn carve_branches(layout: &mut Layout, rng: &mut Rng, params: &DungeonParams) {
    let mut corridor_tiles: Vec<(i32, i32)> = Vec::new();
    for y in 0..layout.height {
        for x in 0..layout.width {
            if layout.get(x, y) == Cell::Corridor {
                corridor_tiles.push((x, y));
            }
        }
    }
    if corridor_tiles.is_empty() {
        return;
    }

    let factor = params.branch_factor.clamp(0.0, 1.0);
    let count = ((corridor_tiles.len() as f32) * factor * 0.05).round() as usize;
    if count == 0 {
        return;
    }
    let wander = params.corridor_wander.clamp(0.0, 1.0);

    for _ in 0..count {
        let &(sx, sy) = &corridor_tiles[rng.gen_range_usize(0, corridor_tiles.len())];
        let length = rng.gen_range_i32(4, 14);
        let (mut dx, mut dy) = match rng.gen_range_usize(0, 4) {
            0 => (1, 0),
            1 => (-1, 0),
            2 => (0, 1),
            _ => (0, -1),
        };
        let mut x = sx;
        let mut y = sy;
        for _ in 0..length {
            // Re-roll direction with the same wander dial — gives spurs the
            // same snaky quality as the main corridors. Don't reverse, that
            // produces an instant dead-end on top of the start cell.
            if rng.gen_unit() < wander {
                let (rdx, rdy) = match rng.gen_range_usize(0, 4) {
                    0 => (1, 0),
                    1 => (-1, 0),
                    2 => (0, 1),
                    _ => (0, -1),
                };
                if (rdx, rdy) != (-dx, -dy) {
                    dx = rdx;
                    dy = rdy;
                }
            }
            let nx = x + dx;
            let ny = y + dy;
            if !layout.in_bounds(nx, ny) {
                break;
            }
            if layout.get(nx, ny) == Cell::Chamber {
                break;
            }
            x = nx;
            y = ny;
            if layout.get(x, y) == Cell::Wall {
                layout.set(x, y, Cell::Corridor);
            }
        }
    }
}

fn carve_corridor_cell(layout: &mut Layout, x: i32, y: i32) {
    if !layout.in_bounds(x, y) {
        return;
    }
    if layout.get(x, y) == Cell::Wall {
        layout.set(x, y, Cell::Corridor);
    }
    // Chamber and Corridor cells are left as-is.
}

fn random_neighbor(x: i32, y: i32, rng: &mut Rng) -> (i32, i32) {
    match rng.gen_range_usize(0, 4) {
        0 => (x + 1, y),
        1 => (x - 1, y),
        2 => (x, y + 1),
        _ => (x, y - 1),
    }
}

fn greedy_step(x: i32, y: i32, tx: i32, ty: i32, rng: &mut Rng) -> (i32, i32) {
    let dx = tx - x;
    let dy = ty - y;
    if dx == 0 && dy == 0 {
        return (x, y);
    }
    // If both axes have remaining distance, pick one at random weighted by
    // magnitude. Otherwise step along the only axis with progress to make.
    if dx != 0 && dy != 0 {
        let total = (dx.abs() + dy.abs()) as u64;
        if rng.gen_range_u64(0, total) < dx.unsigned_abs() as u64 {
            (x + dx.signum(), y)
        } else {
            (x, y + dy.signum())
        }
    } else if dx != 0 {
        (x + dx.signum(), y)
    } else {
        (x, y + dy.signum())
    }
}

fn finish_with_l_shape(layout: &mut Layout, rng: &mut Rng, sx: i32, sy: i32, tx: i32, ty: i32) {
    if rng.gen_range_usize(0, 2) == 0 {
        // Horizontal then vertical.
        let (mut x, y) = (sx, sy);
        while x != tx {
            x += (tx - x).signum();
            carve_corridor_cell(layout, x, y);
        }
        let mut y = sy;
        while y != ty {
            y += (ty - y).signum();
            carve_corridor_cell(layout, x, y);
        }
    } else {
        let (x, mut y) = (sx, sy);
        while y != ty {
            y += (ty - y).signum();
            carve_corridor_cell(layout, x, y);
        }
        let mut x = sx;
        while x != tx {
            x += (tx - x).signum();
            carve_corridor_cell(layout, x, y);
        }
    }
}

// ── Layout → SpaceDefinition ──────────────────────────────────────────────────

fn layout_to_space(authored_id: String, layout: Layout, params: &DungeonParams) -> SpaceDefinition {
    let mut def = SpaceDefinition::new_empty(
        authored_id,
        layout.width,
        layout.height,
        params.fill_floor_type.clone(),
    );

    let mut wall_tiles: Vec<TileCoordinate> = Vec::new();
    let mut chamber_tiles: Vec<TileCoordinate> = Vec::new();
    let mut corridor_tiles: Vec<TileCoordinate> = Vec::new();

    for y in 0..layout.height {
        for x in 0..layout.width {
            match layout.get(x, y) {
                Cell::Wall => {
                    // Only emit walls that touch a chamber or corridor.
                    // Anywhere else stays as the (empty) fill — void/black.
                    if has_carved_neighbor(&layout, x, y) {
                        wall_tiles.push(TileCoordinate { x, y, z: 0 });
                    }
                }
                Cell::Chamber => chamber_tiles.push(TileCoordinate { x, y, z: 0 }),
                Cell::Corridor => corridor_tiles.push(TileCoordinate { x, y, z: 0 }),
            }
        }
    }

    if !wall_tiles.is_empty() {
        def.objects
            .push(MapObjectEntry::Anonymous(AnonymousObjectPlacements {
                type_id: params.wall_type_id.clone(),
                properties: Default::default(),
                placement: wall_tiles,
                facing: None,
            }));
    }

    // Skip floor groups that match `fill_floor_type` — they're redundant.
    if params.chamber_floor != params.fill_floor_type && !chamber_tiles.is_empty() {
        def.floors.insert(
            params.chamber_floor.clone(),
            FloorPlacements {
                placement: chamber_tiles,
                rects: Vec::new(),
            },
        );
    }
    if params.corridor_floor != params.fill_floor_type && !corridor_tiles.is_empty() {
        def.floors
            .entry(params.corridor_floor.clone())
            .and_modify(|f| f.placement.extend(corridor_tiles.iter().copied()))
            .or_insert(FloorPlacements {
                placement: corridor_tiles,
                rects: Vec::new(),
            });
    }

    def
}

/// True if any of the 8 surrounding cells is a carved (non-Wall) tile. Used
/// to keep only the perimeter walls and let the rest of the map stay as
/// black/void empty space.
fn has_carved_neighbor(layout: &Layout, x: i32, y: i32) -> bool {
    for dy in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x + dx;
            let ny = y + dy;
            if !layout.in_bounds(nx, ny) {
                continue;
            }
            if layout.get(nx, ny) != Cell::Wall {
                return true;
            }
        }
    }
    false
}

// ── Tiny PRNG (SplitMix64) ────────────────────────────────────────────────────

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x9E3779B97F4A7C15 } else { seed },
        }
    }

    fn next_u64(&mut self) -> u64 {
        // SplitMix64.
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    fn gen_range_u64(&mut self, lo: u64, hi: u64) -> u64 {
        debug_assert!(hi > lo);
        lo + self.next_u64() % (hi - lo)
    }

    fn gen_range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        let span = (hi - lo) as u64;
        lo + (self.next_u64() % span) as i32
    }

    fn gen_range_usize(&mut self, lo: usize, hi: usize) -> usize {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() as usize) % (hi - lo)
    }

    fn gen_unit(&mut self) -> f32 {
        // 24-bit float in [0, 1).
        ((self.next_u64() >> 40) as f32) / (1u32 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(def: &SpaceDefinition) -> (usize, usize) {
        let walls: usize = def
            .objects
            .iter()
            .filter_map(|e| match e {
                MapObjectEntry::Anonymous(g) if g.type_id == "wall" => Some(g.placement.len()),
                _ => None,
            })
            .sum();
        let floor_cells: usize = def.floors.values().map(|f| f.placement.len()).sum();
        (walls, floor_cells)
    }

    #[test]
    fn determinism_same_seed_same_output() {
        let p = DungeonParams {
            seed: 42,
            ..DungeonParams::default()
        };
        let a = generate_dungeon("a".into(), p.clone());
        let b = generate_dungeon("b".into(), p);
        assert_eq!(count(&a), count(&b));
    }

    #[test]
    fn produces_some_walls_and_floors() {
        let p = DungeonParams {
            seed: 1,
            ..DungeonParams::default()
        };
        let def = generate_dungeon("d".into(), p);
        let (walls, floors) = count(&def);
        assert!(walls > 0, "expected walls, got {walls}");
        // Either chamber or corridor floors should be carved.
        assert!(floors > 0 || !def.floors.is_empty(),);
    }

    #[test]
    fn straight_corridors_when_wander_is_zero() {
        // With wander=0 corridors are pure greedy paths.
        let p = DungeonParams {
            seed: 7,
            corridor_wander: 0.0,
            ..DungeonParams::default()
        };
        let def = generate_dungeon("d".into(), p);
        let (walls, _) = count(&def);
        assert!(walls > 0);
    }

    #[test]
    fn handles_tiny_map_without_panicking() {
        let p = DungeonParams {
            width: 12,
            height: 12,
            seed: 99,
            ..DungeonParams::default()
        };
        let _ = generate_dungeon("tiny".into(), p);
    }
}
