varying highp vec3 wpos;
uniform sampler2D Texture;
uniform mat4 SVP;
uniform vec4 sh_col;

void main() {
    vec4 q = SVP * vec4(wpos, 1.0);
    vec2 spos = q.xy / q.w * 0.5 + 0.5;
    if (spos.x < 0.01 || spos.x > 0.99 || spos.y < 0.01 || spos.y > 0.99) discard;

    // 9-tap Poisson-disk rotated PCF soft shadow decal sampling
    float texel = 1.0 / 2048.0;
    float rot_ang = hash(wpos.xz) * 6.2831853;
    float c = cos(rot_ang), s = sin(rot_ang);
    mat2 rot = mat2(c, -s, s, c);

    vec2 poisson[9];
    poisson[0] = vec2(0.0, 0.0);
    poisson[1] = vec2(-0.94201624, -0.39906216);
    poisson[2] = vec2(0.94558609, -0.76890725);
    poisson[3] = vec2(-0.09418410, -0.92938870);
    poisson[4] = vec2(0.34495938, 0.29387760);
    poisson[5] = vec2(-0.91588581, 0.45771432);
    poisson[6] = vec2(-0.81544232, -0.87912464);
    poisson[7] = vec2(-0.38277543, 0.27676845);
    poisson[8] = vec2(0.97484398, 0.75648379);

    float acc = 0.0;
    for (int i = 0; i < 9; i++) {
        vec2 uv = clamp(spos + rot * poisson[i] * texel * 2.2, 0.001, 0.999);
        acc += (1.0 - texture2D(Texture, uv).r);
    }
    float a = acc / 9.0;
    if (a < 0.015) discard;

    vec2 b = abs(spos - 0.5) * 2.0;
    float fade = 1.0 - smoothstep(0.82, 0.98, max(b.x, b.y));
    gl_FragColor = vec4(sh_col.rgb, a * sh_col.a * fade);
}
