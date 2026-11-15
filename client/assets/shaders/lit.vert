#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
varying highp vec2 uv;
varying lowp vec4 col;
varying highp vec3 wpos;
varying highp vec3 wnrm;
varying highp vec3 v_lpos;
varying highp float spec;
uniform mat4 VP;
uniform mat4 MODEL;
uniform mat4 CarM;
uniform vec3 tint;
void main() {
    mat4 M = MODEL * CarM;
    vec4 wp = M * vec4(position, 1.0);
    wpos = wp.xyz;
    v_lpos = position;
    wnrm = (M * vec4(normal.xyz, 0.0)).xyz;
    spec = normal.w;
    col = (color0 / 255.0) * vec4(tint, 1.0);
    uv = texcoord;
    gl_Position = VP * wp;
}
