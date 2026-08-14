// Renders an I420 (planar YUV 4:2:0) video frame.
//
// The three planes arrive as separate R8Unorm textures. iced sets the render
// pass viewport to the widget bounds before calling us, so clip space *is* the
// widget: a full-clip-space triangle needs no vertex buffer and no transform.

struct Uniforms {
    // Letterbox factor. 1.0 on the axis that fills the widget, < 1.0 on the
    // axis with spare room, which pushes the UV outside [0, 1] to make bars.
    scale: vec2<f32>,
    // 1 when the surface format carries an Srgb suffix, so the GPU will apply
    // the sRGB transfer function on write and we must hand it linear values.
    srgb: u32,
    // Frame rotation, in units of 90 degrees clockwise.
    rotation: u32,
}

@group(0) @binding(0) var y_plane: texture_2d<f32>;
@group(0) @binding(1) var u_plane: texture_2d<f32>;
@group(0) @binding(2) var v_plane: texture_2d<f32>;
@group(0) @binding(3) var plane_sampler: sampler;
@group(0) @binding(4) var<uniform> uniforms: Uniforms;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    // Oversized triangle covering clip space: (-1,-1), (3,-1), (-1,3).
    let corner = vec2<f32>(
        f32((index << 1u) & 2u),
        f32(index & 2u),
    );
    let position = corner * 2.0 - 1.0;

    // Clip space has +y up, textures have +v down.
    let display_uv = vec2<f32>(
        (position.x + 1.0) * 0.5,
        (1.0 - position.y) * 0.5,
    );

    // Work from the centre so scaling and rotation are about the middle.
    var centred = (display_uv - 0.5) / uniforms.scale;

    // Map display space back to texture space. The CPU side already swapped
    // width/height when computing `scale` for the 90/270 cases.
    switch uniforms.rotation {
        case 1u: { centred = vec2<f32>(centred.y, -centred.x); }
        case 2u: { centred = vec2<f32>(-centred.x, -centred.y); }
        case 3u: { centred = vec2<f32>(-centred.y, centred.x); }
        default: {}
    }

    var out: VertexOutput;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = centred + 0.5;
    return out;
}

fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    let low = color / 12.92;
    let high = pow((color + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, color <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Branchless mask rather than an early return: `textureSample` requires
    // uniform control flow, and this keeps the whole shader uniform. Sampling
    // outside [0, 1] is harmless because the sampler clamps to edge.
    let inside = f32(
        all(in.uv >= vec2<f32>(0.0)) && all(in.uv <= vec2<f32>(1.0))
    );

    // Level 0 explicitly: the plane textures have no mips.
    let y = textureSampleLevel(y_plane, plane_sampler, in.uv, 0.0).r;
    let u = textureSampleLevel(u_plane, plane_sampler, in.uv, 0.0).r;
    let v = textureSampleLevel(v_plane, plane_sampler, in.uv, 0.0).r;

    // BT.601, limited range (Y in 16..235, chroma centred on 128).
    let yuv = vec3<f32>(y - 0.0625, u - 0.5, v - 0.5);
    let matrix = mat3x3<f32>(
        vec3<f32>(1.164, 1.164, 1.164),
        vec3<f32>(0.0, -0.392, 2.017),
        vec3<f32>(1.596, -0.813, 0.0),
    );

    var rgb = clamp(matrix * yuv, vec3<f32>(0.0), vec3<f32>(1.0));

    if uniforms.srgb != 0u {
        rgb = srgb_to_linear(rgb);
    }

    // Premultiplied, to match the blend state.
    return vec4<f32>(rgb * inside, inside);
}
