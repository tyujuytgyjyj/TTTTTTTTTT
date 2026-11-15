float hash(vec2 p){ return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float vnoise(vec2 p){
    vec2 i = floor(p); vec2 f = fract(p); f = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y);
}
float fbm(vec2 p){
    float a = 0.5; float s = 0.0;
    for (int k = 0; k < 4; k++) { s += a * vnoise(p); p = p * 2.03 + vec2(17.0, 9.0); a *= 0.5; }
    return s;
}

// Physically-inspired Rayleigh + Mie atmospheric sky with realistic Sunrise, Golden Hour, Sunset, Twilight & Starry Night
vec3 skyc(vec3 d, vec3 sun, float t, bool clouds){
    float y = max(d.y, 0.001);
    float sy = sun.y; // -1.0 (midnight) .. 0.0 (horizon sunrise/sunset) .. +0.88 (noon)

    // Blend factors for Day, Golden-Hour/Twilight, and Night
    float day_f = smoothstep(-0.08, 0.28, sy);
    float twilight_f = smoothstep(-0.22, 0.02, sy) * (1.0 - smoothstep(0.04, 0.38, sy));
    float night_f = 1.0 - smoothstep(-0.18, 0.04, sy);

    // Zenith color: crisp Rayleigh blue by day -> deep royal indigo at sunset -> midnight cosmos at night
    vec3 zen_day = vec3(0.045, 0.19, 0.58);
    vec3 zen_twi = vec3(0.035, 0.055, 0.20);
    vec3 zen_ngt = vec3(0.006, 0.010, 0.028);
    vec3 zen = mix(zen_ngt, zen_day, day_f);
    zen = mix(zen, zen_twi, twilight_f * 0.85);

    // Base horizon color: atmospheric haze by day -> warm amber/rose/violet at sunset -> deep navy at night
    vec3 sun_flat = normalize(vec3(sun.x, 0.0, sun.z) + vec3(1e-5, 0.0, 0.0));
    vec3 d_flat = normalize(vec3(d.x, 0.0, d.z) + vec3(1e-5, 0.0, 0.0));
    float azim = dot(d_flat, sun_flat) * 0.5 + 0.5; // 1.0 toward sun horizon, 0.0 opposite horizon (Belt of Venus)

    vec3 hor_day = vec3(0.52, 0.72, 0.95);
    vec3 hor_ngt = vec3(0.018, 0.030, 0.068);
    // Sunset/Sunrise directional horizon: fiery crimson-gold toward the sun, dusty magenta-violet opposite (Belt of Venus)
    vec3 hor_sun_side = vec3(1.55, 0.36, 0.07);
    vec3 hor_anti_side = vec3(0.32, 0.18, 0.38);
    vec3 hor_twi = mix(hor_anti_side, hor_sun_side, pow(azim, 1.6));

    vec3 hor = mix(hor_ngt, hor_day, day_f);
    hor = mix(hor, hor_twi, twilight_f);

    // Multi-band Rayleigh/Mie vertical gradient (with golden middle band during sunrise/sunset)
    vec3 c = mix(hor, zen, pow(y, 0.42));
    vec3 mid_glow = vec3(1.15, 0.55, 0.16) * pow(azim, 2.2) * exp(-y * 4.5) * (1.0 - exp(-y * 18.0));
    c += mid_glow * twilight_f * 1.35;

    // Solar Mie scattering (Henyey-Greenstein-inspired forward scatter + solar disc)
    float sd = max(dot(d, sun), 0.0);
    float sun_vis = smoothstep(-0.08, 0.02, sy);
    vec3 mie_col = mix(vec3(1.65, 0.42, 0.09), vec3(1.05, 0.88, 0.65), smoothstep(0.0, 0.35, sy));
    c += mie_col * pow(sd, 4.0) * (0.24 + 0.45 * twilight_f) * sun_vis;
    c += mie_col * pow(sd, 32.0) * (0.55 + 0.65 * twilight_f) * sun_vis;
    c += vec3(2.8, 2.2, 1.6) * smoothstep(0.9993, 0.99985, sd) * 3.5 * sun_vis;

    // Moon & Starfield at night
    vec3 moon_dir = normalize(vec3(-sun.x, max(-sun.y, 0.18), -sun.z));
    float md = max(dot(d, moon_dir), 0.0);
    c += vec3(0.25, 0.38, 0.65) * pow(md, 18.0) * 0.35 * night_f;
    c += vec3(1.4, 1.55, 1.8) * smoothstep(0.99945, 0.99985, md) * 1.8 * night_f;

    if (clouds && night_f > 0.02 && d.y > 0.04) {
        vec2 sp = d.xz / (d.y + 0.35) * 160.0;
        vec2 si = floor(sp);
        vec2 sf = fract(sp) - 0.5;
        float sh = hash(si);
        if (sh > 0.965) {
            float r2 = dot(sf, sf);
            float twinkle = 0.65 + 0.35 * sin(t * 2.5 + sh * 90.0);
            float star = exp(-r2 * 55.0) * smoothstep(0.965, 0.999, sh) * twinkle;
            vec3 scol = mix(vec3(0.75, 0.85, 1.0), vec3(1.0, 0.9, 0.75), fract(sh * 37.0));
            c += scol * star * night_f * smoothstep(0.04, 0.28, d.y) * 1.6;
        }
    }

    // Volumetric-lit multi-layer clouds
    if (clouds && d.y > 0.005) {
        vec2 p = d.xz / (d.y + 0.13) * 0.80 + vec2(t * 0.009, t * 0.0035);
        float n = fbm(p * 1.25);
        float cov = smoothstep(0.46, 0.79, n);
        if (cov > 0.005) {
            float dens = fbm(p * 3.2 + 2.7);
            float grad = fbm(p * 1.25 + sun_flat.xz * 0.08) - n;
            float self_sh = clamp(0.55 + grad * 2.8, 0.15, 1.0);

            vec3 cld_top_day = vec3(1.02, 0.99, 0.96);
            vec3 cld_bot_day = vec3(0.55, 0.63, 0.75);
            vec3 cld_top_twi = vec3(1.35, 0.48, 0.16);
            vec3 cld_bot_twi = vec3(0.16, 0.12, 0.25);
            vec3 cld_top_ngt = vec3(0.05, 0.07, 0.13);
            vec3 cld_bot_ngt = vec3(0.015, 0.022, 0.045);

            vec3 ctop = mix(cld_top_ngt, cld_top_day, day_f);
            ctop = mix(ctop, cld_top_twi, twilight_f);
            vec3 cbot = mix(cld_bot_ngt, cld_bot_day, day_f);
            cbot = mix(cbot, cld_bot_twi, twilight_f);

            vec3 cc = mix(cbot, ctop, clamp(dens * self_sh, 0.0, 1.0));
            // Silver/golden lining near sun
            cc += mie_col * pow(sd, 5.0) * (1.1 - dens * 0.6) * (0.45 + 0.75 * twilight_f) * sun_vis;
            c = mix(c, cc, cov * smoothstep(0.005, 0.20, d.y) * 0.92);
        }
    }

    if (d.y < 0.0) {
        vec3 gnd_c = mix(vec3(0.01, 0.015, 0.03), vec3(0.22, 0.26, 0.22), day_f);
        gnd_c = mix(gnd_c, vec3(0.25, 0.12, 0.08), twilight_f);
        c = mix(hor, gnd_c, clamp(-d.y * 6.5, 0.0, 1.0));
    }
    return max(c, vec3(0.0));
}

// ACES Filmic Tonemapping with rich contrast and natural highlight roll-off
vec3 tonemap(vec3 x){
    x *= 1.08;
    vec3 a = x * (2.51 * x + 0.03);
    vec3 b = x * (2.43 * x + 0.59) + 0.14;
    return pow(clamp(a / b, 0.0, 1.0), vec3(1.0 / 2.2));
}
