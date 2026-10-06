// Instanced textured quads. One instance = destination rect (pixels, y down), uv rect and a PREMULTIPLIED tint.
// Textures are uploaded premultiplied; output is blended with ONE / ONE_MINUS_SRC_ALPHA on a non-sRGB target.
struct Frame { viewport: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VIn {
    @builtin(vertex_index) vi: u32,
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) tint: vec4<f32>,
};
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) tint: vec4<f32>,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let c = corners[in.vi];
    let p = in.rect.xy + c * in.rect.zw;
    var o: VOut;
    o.pos = vec4<f32>(p.x / frame.viewport.x * 2.0 - 1.0, 1.0 - p.y / frame.viewport.y * 2.0, 0.0, 1.0);
    o.uv = mix(in.uv.xy, in.uv.zw, c);
    o.tint = in.tint;
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv) * in.tint;
}

// Signed-distance-field glyphs (the 2.9.1 fonts): the distance is stored in alpha, the outline is at 0.5.
@fragment
fn fs_sdf(in: VOut) -> @location(0) vec4<f32> {
    let s = textureSample(tex, samp, in.uv);
    // red = inner (fill) distance, alpha = outer (outline) distance; the plain fonts keep red at 1
    let wf = clamp(fwidth(s.r) * 0.7, 0.01, 0.25);
    let wo = clamp(fwidth(s.a) * 0.7, 0.01, 0.25);
    let coverage = smoothstep(0.5 - wf, 0.5 + wf, s.r) * smoothstep(0.5 - wo, 0.5 + wo, s.a);
    return in.tint * coverage;
}
