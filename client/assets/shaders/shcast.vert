#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
uniform mat4 SVP;
uniform mat4 MODEL;
uniform vec3 sun_dir;
uniform vec4 misc; // y = ground height
void main() {
    vec3 p = (MODEL * vec4(position, 1.0)).xyz;
    float h = max(p.y - misc.y, 0.0);
    p -= sun_dir * (h / max(sun_dir.y, 0.20));
    gl_Position = SVP * vec4(p, 1.0);
}
