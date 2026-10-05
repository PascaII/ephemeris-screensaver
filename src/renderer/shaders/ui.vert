#version 330 core
layout(location = 0) in vec2 a_pos;   // pixels, origin top-left
layout(location = 1) in vec2 a_uv;    // atlas uv (glyphs) or local pixels from the shape centre
layout(location = 2) in vec4 a_color;
layout(location = 3) in vec4 a_param; // x: mode, yzw: shape parameters

uniform vec2 u_res;

out vec2 v_uv;
out vec4 v_color;
flat out vec4 v_param;

void main() {
    v_uv = a_uv;
    v_color = a_color;
    v_param = a_param;
    vec2 p = a_pos / u_res * 2.0 - 1.0;
    gl_Position = vec4(p.x, -p.y, 0.0, 1.0);
}
