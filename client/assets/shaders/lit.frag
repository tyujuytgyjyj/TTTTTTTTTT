varying highp vec2 uv;
varying lowp vec4 col;
varying highp vec3 wpos;
varying highp vec3 wnrm;
varying highp vec3 v_lpos;
varying highp float spec;
uniform sampler2D Texture;
uniform vec3 sun_dir;
uniform vec3 cam_pos;
uniform vec3 fog_col;
uniform vec4 misc;     // x = time, y = true sun elevation (-1..+1), z = headlight_pos.x, w = headlight_pos.z
uniform vec4 head_dir; // xyz = headlight forward dir, w = headlight_pos.y

void main() {
    vec4 tex = texture2D(Texture, uv) * col;
    // Edge Anti-Aliasing on alpha-masked geometry (fences, foliage, cutouts)
    if (col.a > 0.99) {
        if (tex.a < 0.18) discard;
        tex.a = smoothstep(0.18, 0.68, tex.a);
    } else {
        if (tex.a < 0.03) discard;
    }

    vec3 base = pow(max(tex.rgb, vec3(0.0)), vec3(2.2));
    vec3 n = normalize(wnrm);
    vec3 v = normalize(cam_pos - wpos);
    if (dot(n, v) < 0.0) n = -n;

    float sy = misc.y; // True solar elevation: >0 day, ~0 sunrise/sunset, <0 night
    float day_f = smoothstep(-0.06, 0.26, sy);
    float twi_f = smoothstep(-0.18, 0.03, sy) * (1.0 - smoothstep(0.04, 0.35, sy));
    float ngt_f = 1.0 - smoothstep(-0.14, 0.03, sy);

    vec3 L = normalize(sun_dir);
    vec3 true_sun = normalize(vec3(sun_dir.x, sy, sun_dir.z));

    vec3 sun_noon = vec3(2.25, 2.05, 1.82);
    vec3 sun_gold = vec3(2.85, 0.92, 0.24);
    vec3 moon_col = vec3(0.18, 0.28, 0.52);
    vec3 key_col = mix(moon_col, sun_noon, day_f);
    key_col = mix(key_col, sun_gold, twi_f);

    vec3 sky_amb = mix(vec3(0.025, 0.042, 0.095), vec3(0.22, 0.34, 0.58), day_f);
    sky_amb = mix(sky_amb, vec3(0.32, 0.18, 0.36), twi_f);
    vec3 gnd_amb = mix(vec3(0.012, 0.015, 0.025), vec3(0.09, 0.08, 0.065), day_f);
    gnd_amb = mix(gnd_amb, vec3(0.24, 0.09, 0.04), twi_f);

    float eff_spec = clamp(spec, 0.04, 1.0);
    bool is_car_paint = (spec >= 1.8 && spec < 2.95);
    bool is_tyre = (spec < -0.5);
    bool is_carbon_int = (spec >= 2.95 && spec < 3.45);
    bool is_alcantara_int = (spec >= 3.45);

    // -------------------------------------------------------------------------
    // Task 5.3: Procedural 3D Tyre Tread & Sidewall Normal + Parallax Mapping
    // -------------------------------------------------------------------------
    if (is_tyre) {
        float ang = atan(v_lpos.y, v_lpos.z);
        float rad = length(v_lpos.yz);
        float ax = v_lpos.x;
        // Parallax offset along view vector
        vec2 tread_uv = vec2(ang * 14.0, ax * 18.0) + v.xz * 0.08;
        // 4 deep circumferential drainage channels + V-shaped directional tread blocks + micro-sipes
        float circum_groove = smoothstep(0.18, 0.32, abs(fract(ax * 9.5 + 0.5) - 0.5));
        float chevron = sin(tread_uv.x * 3.0 + abs(ax) * 22.0);
        float lat_block = smoothstep(-0.25, 0.20, chevron);
        float sipe = 0.88 + 0.12 * sin(tread_uv.x * 18.0 - ax * 40.0);
        // Sidewall transition: outer radial crown vs vertical sidewall ribs
        float crown_mask = clamp((rad - 0.68) * 5.5, 0.0, 1.0);
        float sw_rib = sin(ang * 48.0) * sin(rad * 85.0) * (1.0 - crown_mask);
        float height = mix(0.5 + 0.25 * sw_rib, circum_groove * (0.35 + 0.65 * lat_block) * sipe, crown_mask);
        // Perturb normal from procedural tread height derivatives
        vec3 n_pert = vec3(
            cos(ax * 9.5 * 6.28318) * 0.32 * crown_mask,
            sin(ang * 42.0) * 0.24,
            cos(ang * 42.0) * 0.24 + sw_rib * 0.28
        );
        n = normalize(n + n_pert * 0.55);
        // Vulcanized rubber albedo & variable roughness (matte sidewall vs slightly sheen tread blocks)
        base = mix(vec3(0.018, 0.019, 0.021), vec3(0.038, 0.040, 0.044), height);
        eff_spec = mix(0.06, 0.22, height * crown_mask);
    }

    // -------------------------------------------------------------------------
    // Task 5.4: Interior Carbon Fiber Weave & Alcantara / Leather Microfiber
    // -------------------------------------------------------------------------
    if (is_carbon_int) {
        vec2 cuv = (v_lpos.xz + v_lpos.xy) * 140.0;
        float weave = sin(cuv.x + cuv.y) * cos(cuv.x - cuv.y);
        n = normalize(n + vec3(weave * 0.14, 0.0, -weave * 0.14));
        base = mix(vec3(0.025, 0.027, 0.030), vec3(0.075, 0.080, 0.090), weave * 0.5 + 0.5);
        eff_spec = 0.58 + 0.22 * weave;
    } else if (is_alcantara_int) {
        float micro = vnoise(v_lpos.xz * 95.0 + v_lpos.xy * 95.0);
        float velvet_rim = pow(1.0 - max(dot(n, v), 0.0), 2.4);
        base = base * (0.85 + 0.15 * micro) + vec3(0.04, 0.042, 0.048) * velvet_rim;
        eff_spec = 0.10;
    }

    float ndl = max(dot(n, L), 0.0);
    float wrap_ndl = max((dot(n, L) + 0.18) / 1.18, 0.0);
    float ndv = max(dot(n, v), 0.001);
    vec3 hemi_amb = mix(gnd_amb, sky_amb, clamp(n.y * 0.5 + 0.5, 0.0, 1.0));
    // Multi-Bounce Hemisphere GI + Ground Contact Ambient Occlusion (SSAO/Contact AO approximation)
    float ground_contact_ao = smoothstep(0.0, 0.65, max(wpos.y, 0.0)) * 0.32 + 0.68;
    float cavity_ao = 0.82 + 0.18 * clamp(n.y * 0.6 + 0.4, 0.0, 1.0);
    float ao = clamp(ground_contact_ao * cavity_ao, 0.52, 1.05);
    vec3 gi_bounce = gnd_amb * max(-n.y, 0.0) * 0.42 * day_f;

    vec3 colr;
    // -------------------------------------------------------------------------
    // Task 5.2: Dual-Layer Automotive Clear Coat + Metallic Flake PBR Shader
    // -------------------------------------------------------------------------
    if (is_car_paint) {
        // Metallic flake micro-sparkle in car body space
        float flake = hash(floor(v_lpos.xz * 180.0 + v_lpos.xy * 110.0));
        vec3 flake_n = normalize(n + vec3(flake - 0.5, 0.0, fract(flake * 17.3) - 0.5) * 0.055);
        vec3 r = reflect(-v, n);
        // Horizon specular occlusion: downward reflections reflect dark road asphalt instead of bright sky
        float hor_occ = smoothstep(-0.22, 0.14, r.y);
        vec3 env_sky = pow(skyc(vec3(r.x, max(r.y, 0.01), r.z), true_sun, misc.x, false), vec3(2.2));
        vec3 env_gnd = gnd_amb * 0.55;
        vec3 env_refl = mix(env_gnd, env_sky, hor_occ);

        // Layer 1: Metallic Base Coat (Roughness ~0.24, F0 tinted by body paint)
        float metallic = 0.80;
        vec3 f0_base = mix(vec3(0.04), base, metallic);
        vec3 fres_base = f0_base + (vec3(1.0) - f0_base) * pow(1.0 - ndv, 5.0);
        vec3 h = normalize(v + L);
        float spec_base = pow(max(dot(flake_n, h), 0.0), 42.0) * ndl * (0.85 + 0.35 * step(0.82, flake));
        vec3 diffuse_coat = base * (1.0 - metallic) * ((hemi_amb + gi_bounce) * ao + key_col * wrap_ndl);
        vec3 metallic_coat = env_refl * fres_base * 0.65 + key_col * f0_base * spec_base * 1.45;

        // Layer 2: Dielectric Clear Coat (F0 = 0.04, Ultra-sharp Roughness ~0.035)
        float fres_cc = 0.04 + 0.96 * pow(1.0 - ndv, 5.0);
        float spec_cc = pow(max(dot(n, h), 0.0), 380.0) * ndl;
        vec3 clear_coat = env_refl * fres_cc * 1.15 + key_col * spec_cc * 2.6;

        colr = (diffuse_coat + metallic_coat) * (1.0 - fres_cc * 0.65) + clear_coat;
    } else {
        vec3 diff = base * ((hemi_amb + gi_bounce) * ao + key_col * mix(ndl, wrap_ndl, 0.25));
        float rim = pow(1.0 - ndv, 3.2) * pow(max(dot(-v, L), 0.0), 2.5);
        diff += base * key_col * rim * (0.22 + 0.55 * twi_f);

        float fres = 0.04 + 0.96 * pow(1.0 - ndv, 4.5);
        vec3 r = reflect(-v, n);
        float hor_occ = smoothstep(-0.25, 0.10, r.y);
        vec3 refl = mix(gnd_amb * 0.5, pow(skyc(vec3(r.x, max(r.y, 0.01), r.z), true_sun, misc.x, false), vec3(2.2)), hor_occ);
        vec3 h = normalize(v + L);
        float shininess = mix(28.0, 160.0, clamp(eff_spec, 0.0, 1.0));
        float sp = pow(max(dot(n, h), 0.0), shininess) * eff_spec * ndl;

        colr = diff * (1.0 - 0.24 * eff_spec) + refl * (fres * eff_spec * 0.85) + key_col * sp * 1.15;
    }

    // Player Car Dual Xenon/LED Headlights & Warm Street/Window Glow at Evening/Night
    if (ngt_f > 0.01 || twi_f > 0.25) {
        float night_strength = clamp(ngt_f + twi_f * 0.55, 0.0, 1.0);
        vec3 hp = vec3(misc.z, head_dir.w, misc.w);
        vec3 hd = normalize(head_dir.xyz);
        vec3 to_p = wpos - hp;
        float dist_h = length(to_p);
        if (dist_h > 1.2 && dist_h < 115.0) {
            vec3 dir_h = to_p / dist_h;
            float cone = dot(dir_h, hd);
            if (cone > 0.62) {
                float spot = smoothstep(0.62, 0.93, cone);
                float atten = 1.0 / (1.0 + 0.012 * dist_h + 0.00045 * dist_h * dist_h);
                float h_ndl = max(dot(n, -dir_h), 0.15);
                vec3 h_col = vec3(2.6, 2.35, 1.95) * spot * atten * night_strength * 2.2;
                colr += base * h_col * h_ndl + h_col * pow(max(dot(n, normalize(v - dir_h)), 0.0), 48.0) * eff_spec;
            }
        }
    }

    // Distance Atmospheric Scattering (Aerial Perspective)
    float d = length(cam_pos - wpos);
    float f = 1.0 - exp(-d * 0.00105);
    vec3 view_dir = normalize(wpos - cam_pos);
    vec3 aerial_col = pow(max(fog_col, vec3(0.0)), vec3(2.2));
    float view_sun = max(dot(view_dir, true_sun), 0.0);
    aerial_col += sun_gold * pow(view_sun, 6.0) * 0.35 * (twi_f + day_f * 0.25);
    colr = mix(colr, aerial_col, clamp(f, 0.0, 1.0));

    gl_FragColor = vec4(tonemap(colr), tex.a);
}
