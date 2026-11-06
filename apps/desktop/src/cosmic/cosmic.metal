#include <metal_stdlib>
using namespace metal;

struct CosmicUniforms {
    float2 resolution;
    float time_seconds;
    float bass;
    float mid;
    float high;
    float rms;
    float peak;
};

struct CosmicVertex {
    float4 position [[position]];
    float2 uv;
};

float hash21(float2 point) {
    point = fract(point * float2(123.34, 456.21));
    point += dot(point, point + 45.32);
    return fract(point.x * point.y);
}

float noise21(float2 point) {
    float2 cell = floor(point);
    float2 local = fract(point);
    local = local * local * (3.0 - 2.0 * local);
    float low = mix(hash21(cell), hash21(cell + float2(1.0, 0.0)), local.x);
    float high = mix(
        hash21(cell + float2(0.0, 1.0)),
        hash21(cell + float2(1.0, 1.0)),
        local.x
    );
    return mix(low, high, local.y);
}

float fbm(float2 point) {
    float value = 0.0;
    float amplitude = 0.5;
    float2x2 warp = float2x2(0.80, -0.60, 0.60, 0.80);
    for (uint octave = 0; octave < 5; octave++) {
        value += amplitude * noise21(point);
        point = warp * point * 2.03 + float2(7.1, 3.7);
        amplitude *= 0.5;
    }
    return value;
}

float3 palette(float phase) {
    float3 violet = float3(0.31, 0.03, 0.72);
    float3 magenta = float3(0.96, 0.04, 0.62);
    float3 cyan = float3(0.02, 0.86, 1.00);
    float3 amber = float3(1.00, 0.36, 0.05);
    float3 first = mix(violet, magenta, smoothstep(0.05, 0.48, phase));
    float3 second = mix(cyan, amber, smoothstep(0.58, 1.0, phase));
    return mix(first, second, smoothstep(0.42, 0.76, phase));
}

vertex CosmicVertex cosmic_vertex(uint vertex_id [[vertex_id]]) {
    const float2 positions[3] = {
        float2(-1.0, -1.0),
        float2(3.0, -1.0),
        float2(-1.0, 3.0)
    };
    CosmicVertex output;
    output.position = float4(positions[vertex_id], 0.0, 1.0);
    output.uv = positions[vertex_id] * 0.5 + 0.5;
    return output;
}

fragment float4 cosmic_fragment(
    CosmicVertex input [[stage_in]],
    constant CosmicUniforms& uniforms [[buffer(0)]]
) {
    float2 resolution = max(uniforms.resolution, float2(1.0));
    float2 point = (input.uv * 2.0 - 1.0);
    point.x *= resolution.x / resolution.y;

    float time = uniforms.time_seconds * (0.055 + uniforms.mid * 0.055);
    float radius = length(point);
    float angle = atan2(point.y, point.x);
    float spiral = angle * 1.8 + radius * (5.5 + uniforms.bass * 2.5) - time * 2.4;
    float2 flow = point;
    flow += 0.16 * float2(cos(spiral), sin(spiral));
    flow += uniforms.rms * 0.07 * float2(sin(angle * 5.0), cos(angle * 4.0));

    float cloud_a = fbm(flow * 2.2 + float2(time, -time * 0.7));
    float cloud_b = fbm(flow * 4.4 - float2(time * 0.4, time * 0.9));
    float filament = abs(sin(spiral + cloud_b * 7.0));
    filament = pow(max(0.0, 1.0 - filament), 5.0);
    filament *= 0.35 + uniforms.bass * 1.7 + uniforms.peak * 0.6;

    float nebula = smoothstep(0.18, 0.92, cloud_a + cloud_b * 0.55 - radius * 0.15);
    float phase = fract(cloud_a * 0.65 + cloud_b * 0.4 + angle / 6.28318 + time * 0.08);
    float3 color = palette(phase) * (0.18 + nebula * 0.82);
    color += palette(fract(phase + 0.36)) * filament;

    float2 star_cell = floor(input.uv * resolution / 3.0);
    float star_seed = hash21(star_cell);
    float stars = step(0.9965 - uniforms.high * 0.0015, star_seed);
    stars *= 0.4 + 1.7 * pow(star_seed, 18.0) + uniforms.high * 1.2;
    color += stars * mix(float3(0.35, 0.65, 1.0), float3(1.0, 0.35, 0.85), star_seed);

    float core = exp(-radius * (2.4 - uniforms.rms * 0.5));
    color += core * float3(0.08, 0.03, 0.22) * (0.5 + uniforms.peak);
    color *= 0.72 + uniforms.rms * 0.45;
    color = color / (1.0 + color);
    color = pow(max(color, float3(0.0)), float3(0.82));

    return float4(color, 1.0);
}
