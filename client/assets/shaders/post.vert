#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying highp vec2 uv;
void main() {
    uv = texcoord;
    gl_Position = vec4(position.xy, 0.0, 1.0);
}
