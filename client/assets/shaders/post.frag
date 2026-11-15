#version 100
precision highp float;
varying highp vec2 uv;
uniform sampler2D Texture;
uniform vec4 pp_params; // x: texel_w, y: texel_h, z: smaa_strength, w: cas_sharpness

float luma(vec3 c) {
    return dot(c, vec3(0.299, 0.587, 0.114));
}

void main() {
    vec2 tx = max(pp_params.xy, vec2(1.0 / 3840.0));
    vec3 cM = texture2D(Texture, uv).rgb;
    if (pp_params.z <= 0.01) {
        gl_FragColor = vec4(cM, 1.0);
        return;
    }

    vec3 cN = texture2D(Texture, uv + vec2(0.0, -tx.y)).rgb;
    vec3 cS = texture2D(Texture, uv + vec2(0.0,  tx.y)).rgb;
    vec3 cW = texture2D(Texture, uv + vec2(-tx.x, 0.0)).rgb;
    vec3 cE = texture2D(Texture, uv + vec2( tx.x, 0.0)).rgb;
    vec3 cNW = texture2D(Texture, uv + vec2(-tx.x, -tx.y)).rgb;
    vec3 cNE = texture2D(Texture, uv + vec2( tx.x, -tx.y)).rgb;
    vec3 cSW = texture2D(Texture, uv + vec2(-tx.x,  tx.y)).rgb;
    vec3 cSE = texture2D(Texture, uv + vec2( tx.x,  tx.y)).rgb;

    float lM = luma(cM);
    float lN = luma(cN), lS = luma(cS), lW = luma(cW), lE = luma(cE);
    float lMin = min(lM, min(min(lN, lS), min(lW, lE)));
    float lMax = max(lM, max(max(lN, lS), max(lW, lE)));
    float range = lMax - lMin;

    vec3 result = cM;
    if (range > max(0.0312, lMax * 0.085)) {
        float lNW = luma(cNW), lNE = luma(cNE), lSW = luma(cSW), lSE = luma(cSE);
        vec2 dir;
        dir.x = -((lNW + lNE) - (lSW + lSE));
        dir.y =  ((lNW + lSW) - (lNE + lSE));
        float dirReduce = max((lN + lS + lW + lE) * (0.25 * 0.25), 1.0 / 128.0);
        float rcpDirMin = 1.0 / (min(abs(dir.x), abs(dir.y)) + dirReduce);
        dir = clamp(dir * rcpDirMin, vec2(-6.0, -6.0), vec2(6.0, 6.0)) * tx;

        vec3 rgbA = 0.5 * (
            texture2D(Texture, clamp(uv + dir * (1.0 / 3.0 - 0.5), 0.001, 0.999)).rgb +
            texture2D(Texture, clamp(uv + dir * (2.0 / 3.0 - 0.5), 0.001, 0.999)).rgb
        );
        vec3 rgbB = rgbA * 0.5 + 0.25 * (
            texture2D(Texture, clamp(uv + dir * -0.5, 0.001, 0.999)).rgb +
            texture2D(Texture, clamp(uv + dir *  0.5, 0.001, 0.999)).rgb
        );
        float lB = luma(rgbB);
        vec3 aa_col = (lB < lMin || lB > lMax) ? rgbA : rgbB;
        result = mix(cM, aa_col, clamp(pp_params.z, 0.0, 1.0));
    }

    // Contrast-Adaptive Sharpening (CAS)
    float sharp = clamp(pp_params.w, 0.0, 0.35);
    vec3 blur = (cN + cS + cW + cE) * 0.25;
    result = clamp(result + (result - blur) * sharp, 0.0, 1.0);
    gl_FragColor = vec4(result, 1.0);
}
