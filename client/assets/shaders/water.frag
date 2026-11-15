varying highp vec3 wp;
varying mediump vec4 col;
uniform vec3 sun_dir;
uniform vec3 cam_pos;
uniform vec3 fog_col;
uniform vec4 misc; // x = time, y = true sun elevation

void main() {
    float t = misc.x;
    vec3 true_sun = normalize(vec3(sun_dir.x, misc.y, sun_dir.z));
    vec2 p = wp.xz;

    // Analytical wave gradients match the vertex displacement.
    float q1 = dot(p, vec2(0.6, 0.8)) * 0.11 + t * 1.25;
    float q2 = dot(p, vec2(-0.8, 0.5)) * 0.21 - t * 1.65;
    float q3 = dot(p, vec2(0.3, -0.95)) * 0.42 + t * 2.10;
    vec2 gradient = vec2(0.0);
    gradient += vec2(0.6, 0.8) * cos(q1) * (0.085 * 0.11);
    gradient += vec2(-0.8, 0.5) * cos(q2) * (0.034 * 0.21);
    gradient += vec2(0.3, -0.95) * cos(q3) * (0.014 * 0.42);
    gradient += (vec2(
        vnoise(p * 0.65 + t * 0.045),
        vnoise(p * 0.65 - t * 0.035 + 4.7)
    ) - 0.5) * 0.022;

    vec3 n = normalize(vec3(-gradient.x, 1.0, -gradient.y));
    vec3 vdir = normalize(cam_pos - wp);
    float ndv = max(dot(n, vdir), 0.0);
    float fresnel = 0.02 + 0.98 * pow(1.0 - ndv, 5.0);

    vec3 reflected_dir = reflect(-vdir, n);
    reflected_dir.y = abs(reflected_dir.y);
    vec3 reflection = skyc(reflected_dir, true_sun, t, true);

    // Water absorption: shallow turquoise transitions to deep blue-green.
    float depth = clamp(col.a, 0.0, 1.0);
    float deepness = smoothstep(0.08, 0.96, depth);
    vec3 shallow_water = vec3(0.018, 0.30, 0.32);
    vec3 deep_water = vec3(0.004, 0.045, 0.082);
    vec3 authored_tint = pow(max(col.rgb, vec3(0.006)), vec3(2.2));
    vec3 body_color = mix(shallow_water, deep_water, deepness);
    body_color = mix(body_color, authored_tint, 0.24);

    // Keep the body readable head-on, then let the sky take over at grazing angles.
    vec3 color = mix(body_color, reflection, clamp(0.14 + fresnel * 0.84, 0.0, 0.92));

    // Moving shoreline foam is driven by the depth mask stored in vertex alpha.
    float shore = 1.0 - smoothstep(0.02, 0.62, depth);
    float foam_noise = fbm(p * 0.34 + vec2(t * 0.075, -t * 0.052));
    float foam = smoothstep(0.58, 0.82, foam_noise + shore * 0.56);
    foam *= smoothstep(0.16, 0.78, shore);
    color = mix(color, vec3(0.72, 0.86, 0.84), foam * 0.72);

    // Tight solar glints and a softer secondary highlight on wave crests.
    float sun_glint = pow(max(dot(reflected_dir, true_sun), 0.0), 190.0)
        * smoothstep(-0.05, 0.22, misc.y);
    float crest = pow(max(dot(n, normalize(true_sun + vdir)), 0.0), 72.0);
    color += vec3(2.0, 1.72, 1.38) * sun_glint;
    color += vec3(0.08, 0.16, 0.18) * crest * 0.18;

    // Water haze is intentionally restrained so the surface does not become milky.
    float distance_to_camera = length(cam_pos - wp);
    float haze = 1.0 - exp(-distance_to_camera * 0.00042);
    vec3 aerial = pow(max(fog_col, vec3(0.0)), vec3(2.2));
    color = mix(color, aerial, clamp(haze, 0.0, 0.34));

    gl_FragColor = vec4(tonemap(color), 1.0);
}
