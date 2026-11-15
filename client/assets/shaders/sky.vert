#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying highp vec2 ndc;
void main() { ndc = position.xy; gl_Position = vec4(position.xy, 0.9999, 1.0); }
