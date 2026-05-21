// Fullscreen fog-of-war overlay.
//
// One big quad rendered above the world. The shader maps each fragment's
// world position to a tile, looks up that tile in a packed "discovered" mask
// uploaded each frame, and either renders transparent (discovered) or a deep
// dark starry pattern (undiscovered). The starry pattern is procedural and
// deterministic in world space, so a tile's stars stay put as the camera
// scrolls past — feels like a stable patch of night sky over unexplored land,
// not crawling noise.
//
// The mask layout mirrors `darkness_overlay.wgsl` so the same packing helpers
// apply: a window of `mask_origin.zw` tiles around the player, bits packed
// into `array<vec4<u32>, 80>` (up to 10240 bits, easily covering a 97×97
// window). The window must cover the visible viewport — otherwise discovered
// tiles near the monitor edge would read as undiscovered.
//
// Boundary rendering is soft: `discovered_coverage` bilinearly samples the
// four nearest tile centers and the fragment shader smoothsteps + jitters the
// result so the fog/clear edge feathers across roughly one tile and looks
// fluffy/jagged instead of grid-aligned.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

const MASK_VEC4_COUNT: u32 = 80u;

struct FogUniforms {
    // Tile-to-world transform: world = tile * tile_size + origin.
    // xy: origin of tile (0,0); z: tile_size; w: unused.
    tile_xform: vec4<f32>,
    // xy: window origin tile coords (i32 stored as f32);
    // z: window width, w: window height
    mask_origin: vec4<f32>,
    // Bit `my * window_w + mx` is 1 iff that tile is *discovered*.
    mask: array<vec4<u32>, 80>,
}

@group(2) @binding(0) var<uniform> u: FogUniforms;

fn read_mask_bit(bit_index: u32) -> bool {
    let u32_index = bit_index / 32u;
    let bit_in_u32 = bit_index % 32u;
    let vec4_index = u32_index / 4u;
    let component = u32_index % 4u;
    if (vec4_index >= MASK_VEC4_COUNT) {
        return false;
    }
    let v = u.mask[vec4_index];
    let word = v[component];
    return ((word >> bit_in_u32) & 1u) != 0u;
}

fn sample_discovered(tx: i32, ty: i32) -> f32 {
    let w = i32(u.mask_origin.z);
    let h = i32(u.mask_origin.w);
    let mx = tx - i32(u.mask_origin.x);
    let my = ty - i32(u.mask_origin.y);
    if (mx < 0 || mx >= w || my < 0 || my >= h) {
        return 0.0;
    }
    let bit_index = u32(my * w + mx);
    if (read_mask_bit(bit_index)) {
        return 1.0;
    }
    return 0.0;
}

// Bilinear coverage in [0,1] sampled at the four nearest tile *centers*.
// Interior of a discovered region returns 1.0; interior of an undiscovered
// region returns 0.0; the boundary band spans ~one tile.
fn discovered_coverage(world_xy: vec2<f32>) -> f32 {
    let tile_size = u.tile_xform.z;
    if (tile_size <= 0.0) {
        return 0.0;
    }
    // Tile centers sit at integer tile coords + 0.5 in tile space.
    let tx_f = (world_xy.x - u.tile_xform.x) / tile_size - 0.5;
    let ty_f = (world_xy.y - u.tile_xform.y) / tile_size - 0.5;
    let tx0 = i32(floor(tx_f));
    let ty0 = i32(floor(ty_f));
    let fx = tx_f - f32(tx0);
    let fy = ty_f - f32(ty0);
    let d00 = sample_discovered(tx0,     ty0);
    let d10 = sample_discovered(tx0 + 1, ty0);
    let d01 = sample_discovered(tx0,     ty0 + 1);
    let d11 = sample_discovered(tx0 + 1, ty0 + 1);
    return mix(mix(d00, d10, fx), mix(d01, d11, fx), fy);
}

// Cheap hash-noise: deterministic in world space, near-uniform.
fn hash21(p: vec2<f32>) -> f32 {
    let q = vec2<f32>(
        dot(p, vec2<f32>(127.1, 311.7)),
        dot(p, vec2<f32>(269.5, 183.3)),
    );
    let s = sin(q) * 43758.5453;
    return fract(s.x + s.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let world_xy = in.world_position.xy;

    let coverage = discovered_coverage(world_xy);
    // Sub-tile hash jitter roughens the boundary into teeth/fluff so it
    // doesn't read as a smooth circle around the discovered region.
    let jitter = (hash21(floor(world_xy / 6.0)) - 0.5) * 0.30;
    let fog_alpha = 1.0 - smoothstep(0.35, 0.65, coverage + jitter);
    if (fog_alpha <= 0.001) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    // Quantize to ~3 px cells so stars are visible chunks, not 1-pixel specks
    // at retina densities. 16 px per "star cell" makes stars roughly the size
    // of a sprite pixel at 1:1 zoom.
    let cell = floor(world_xy / 3.0);
    let h = hash21(cell);

    // Base night-sky color: deep indigo. Slight per-cell variation so the
    // background isn't a perfectly flat plane.
    let bg_jitter = 0.015 * hash21(cell * vec2<f32>(0.37, 1.13));
    var color = vec3<f32>(0.020, 0.020, 0.045) + vec3<f32>(bg_jitter);

    // Star draw: ~1 in 256 cells gets a bright pixel, ~1 in 4096 gets a very
    // bright one. Gives the eye something to track without making the field
    // look noisy.
    if (h > 0.996) {
        color = vec3<f32>(0.95, 0.95, 1.0);
    } else if (h > 0.985) {
        color = vec3<f32>(0.55, 0.55, 0.70);
    } else if (h > 0.965) {
        color = vec3<f32>(0.25, 0.25, 0.35);
    }

    return vec4<f32>(color, fog_alpha);
}
