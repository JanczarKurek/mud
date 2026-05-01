// Fullscreen darkness overlay.
//
// Replaces the per-sprite ambient tinting + per-light additive overlays with
// a single big quad rendered on top of the world. The quad's color is the
// space's ambient tint and its alpha is "how dark is this point". Lights
// reduce the alpha (carve holes) so the natural sprite color shows through;
// they never *add* color, so nothing can be brighter than its base sprite.
//
// Per-pixel work: derive the world tile under the fragment, sample a packed
// indoor bitmask to choose outdoor vs. indoor base alpha, then subtract a
// smoothstep falloff from each active light. Clamp at 0 so a saturated light
// just yields full transparency (= natural sprite color), nothing brighter.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

const MAX_LIGHTS: u32 = 32u;
// 16 vec4<u32> = 64 u32 = 2048 bits — covers any 33×33 lighting window with headroom.
const MASK_VEC4_COUNT: u32 = 16u;

struct DarknessUniforms {
    // Outdoor ambient (rgb) and alpha (a)
    outdoor: vec4<f32>,
    // Indoor ambient (rgb) and alpha (a)
    indoor: vec4<f32>,
    // Tile-to-world transform: world = tile * tile_size + origin.
    // xy: world origin of tile (0,0); z: tile_size; w: unused.
    tile_xform: vec4<f32>,
    // xy: bitmask origin tile coords (i32 stored as f32);
    // z: window width, w: window height
    mask_origin: vec4<f32>,
    // x: num_lights (clamped to MAX_LIGHTS); yzw: unused
    counts: vec4<f32>,
    // Indoor bitmask: bit `mask_y * window_w + mask_x` is 1 iff that tile is indoor.
    mask: array<vec4<u32>, 16>,
    // Each light: xy = world position, z = radius (world units), w = intensity
    lights: array<vec4<f32>, 32>,
}

@group(2) @binding(0) var<uniform> u: DarknessUniforms;

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

fn is_indoor(world_xy: vec2<f32>) -> bool {
    let tile_size = u.tile_xform.z;
    if (tile_size <= 0.0) {
        return false;
    }
    let tile_xf = floor((world_xy.x - u.tile_xform.x) / tile_size);
    let tile_yf = floor((world_xy.y - u.tile_xform.y) / tile_size);
    let mask_x = i32(tile_xf - u.mask_origin.x);
    let mask_y = i32(tile_yf - u.mask_origin.y);
    let w = i32(u.mask_origin.z);
    let h = i32(u.mask_origin.w);
    if (mask_x < 0 || mask_x >= w || mask_y < 0 || mask_y >= h) {
        return false;
    }
    let bit_index = u32(mask_y * w + mask_x);
    return read_mask_bit(bit_index);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let world_xy = in.world_position.xy;

    var base_color: vec3<f32>;
    var base_alpha: f32;
    if (is_indoor(world_xy)) {
        base_color = u.indoor.rgb;
        base_alpha = u.indoor.a;
    } else {
        base_color = u.outdoor.rgb;
        base_alpha = u.outdoor.a;
    }

    var alpha = base_alpha;
    let num_lights = u32(u.counts.x);
    let n = min(num_lights, MAX_LIGHTS);
    for (var i: u32 = 0u; i < n; i = i + 1u) {
        let light = u.lights[i];
        let radius = light.z;
        if (radius <= 0.0) {
            continue;
        }
        let dx = world_xy.x - light.x;
        let dy = world_xy.y - light.y;
        let dist = sqrt(dx * dx + dy * dy);
        // smoothstep gives a flat-ish core and gentle edge fade.
        let plateau = radius * 0.18;
        let f = 1.0 - smoothstep(plateau, radius, dist);
        let intensity = light.w;
        alpha = alpha - f * intensity;
    }
    alpha = max(alpha, 0.0);

    return vec4<f32>(base_color, alpha);
}
