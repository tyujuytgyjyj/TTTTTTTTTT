#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying highp vec3 wpos;
uniform mat4 VP;
void main() { wpos = position; gl_Position = VP * vec4(position, 1.0); }
