#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;

varying highp vec3 wp;
varying mediump vec4 col;

uniform mat4 VP;
uniform vec4 misc; // x = time, y = true sun elevation

void main() {
    float t = misc.x;
    vec2 p = position.xz;
    float q1 = dot(p, vec2(0.6, 0.8)) * 0.11 + t * 1.25;
    float q2 = dot(p, vec2(-0.8, 0.5)) * 0.21 - t * 1.65;
    float q3 = dot(p, vec2(0.3, -0.95)) * 0.42 + t * 2.10;

    float depth = clamp(color0.a / 255.0, 0.0, 1.0);
    float shore_fade = smoothstep(0.015, 0.38, depth);
    float wave_height = (
        sin(q1) * 0.085 +
        sin(q2) * 0.034 +
        sin(q3) * 0.014
    ) * shore_fade;

    vec3 displaced = position;
    displaced.y += wave_height;
    wp = displaced;
    col = color0 / 255.0;
    gl_Position = VP * vec4(displaced, 1.0);
}
