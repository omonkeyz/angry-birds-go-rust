// Instanced, vertex-coloured meshes with a hemisphere + sun light and distance fog.
struct Camera {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    fog: vec4<f32>,   // rgb = fog / sky colour, w = density
    sun: vec4<f32>,   // xyz = direction the light travels
};
@group(0) @binding(0) var<uniform> cam: Camera;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) tint: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(in: VIn) -> VOut {
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
    let world = model * vec4<f32>(in.pos, 1.0);
    var out: VOut;
    out.clip = cam.view_proj * world;
    out.world = world.xyz;
    out.normal = (model * vec4<f32>(in.normal, 0.0)).xyz;
    out.color = in.color * in.tint;
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    var n = normalize(in.normal);
    // two-sided: flip the normal to face the camera so back faces are lit like front faces
    let to_eye = normalize(cam.eye.xyz - in.world);
    if (dot(n, to_eye) < 0.0) { n = -n; }
    let sun = max(dot(n, -normalize(cam.sun.xyz)), 0.0);
    let hemi = mix(vec3<f32>(0.34, 0.31, 0.28), vec3<f32>(0.60, 0.68, 0.80), n.y * 0.5 + 0.5);
    var lit = in.color.rgb * (hemi + vec3<f32>(1.0, 0.95, 0.85) * sun * 0.8);
    let dist = distance(in.world, cam.eye.xyz);
    let fog = clamp(1.0 - exp(-dist * cam.fog.w), 0.0, 1.0);
    lit = mix(lit, cam.fog.rgb, fog);
    return vec4<f32>(lit, 1.0);
}

// Textured variant: the same lighting and fog, the colour multiplied by the material texture (alpha-tested for foliage).
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct TVIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(8) uv: vec2<f32>,
    @location(3) m0: vec4<f32>,
    @location(4) m1: vec4<f32>,
    @location(5) m2: vec4<f32>,
    @location(6) m3: vec4<f32>,
    @location(7) tint: vec4<f32>,
};

struct TVOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

@vertex
fn vs_tex(in: TVIn) -> TVOut {
    let model = mat4x4<f32>(in.m0, in.m1, in.m2, in.m3);
    let world = model * vec4<f32>(in.pos, 1.0);
    var out: TVOut;
    out.clip = cam.view_proj * world;
    out.world = world.xyz;
    out.normal = (model * vec4<f32>(in.normal, 0.0)).xyz;
    out.color = in.color * in.tint;
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_tex(in: TVOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    if (t.a < 0.4) { discard; }
    var n = in.normal;
    if (length(n) < 0.5) {
        // the track terrain carries no normals: use the face normal
        n = cross(dpdx(in.world), dpdy(in.world));
        if (n.y < 0.0) { n = -n; }
    }
    n = normalize(n);
    let to_eye = normalize(cam.eye.xyz - in.world);
    if (dot(n, to_eye) < 0.0) { n = -n; }
    let sun = max(dot(n, -normalize(cam.sun.xyz)), 0.0);
    let hemi = mix(vec3<f32>(0.42, 0.40, 0.38), vec3<f32>(0.70, 0.76, 0.86), n.y * 0.5 + 0.5);
    var lit = t.rgb * in.color.rgb * (hemi + vec3<f32>(1.0, 0.95, 0.85) * sun * 0.6);
    let dist = distance(in.world, cam.eye.xyz);
    let fog = clamp(1.0 - exp(-dist * cam.fog.w), 0.0, 1.0);
    lit = mix(lit, cam.fog.rgb, fog);
    return vec4<f32>(lit, 1.0);
}
