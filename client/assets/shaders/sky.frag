varying highp vec2 ndc;
uniform mat4 invVP;
uniform vec3 sun_dir;
uniform vec3 cam_pos;
uniform vec4 misc;
void main() {
    vec4 p = invVP * vec4(ndc, 1.0, 1.0);
    vec3 d = normalize(p.xyz / p.w - cam_pos);
    gl_FragColor = vec4(tonemap(pow(skyc(d, sun_dir, misc.x, true), vec3(2.2)) * 1.05), 1.0);
}
