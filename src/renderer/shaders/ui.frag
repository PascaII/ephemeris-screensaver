#version 330 core
// All 2D overlay primitives, analytically anti-aliased.
//   mode 0: rounded rect  (param.yz = half size, param.w = corner radius)
//   mode 1: glyph         (alpha from the R8 atlas)
//   mode 2: disc          (param.y = radius)
//   mode 3: ring          (param.y = radius, param.z = thickness)
//   mode 4: glow          (param.y = gaussian sigma)

uniform sampler2D u_atlas;

in vec2 v_uv;
in vec4 v_color;
flat in vec4 v_param;
out vec4 frag;

void main() {
    int mode = int(v_param.x + 0.5);
    float a;
    if (mode == 0) {
        vec2 q = abs(v_uv) - v_param.yz + v_param.w;
        float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - v_param.w;
        a = clamp(0.5 - d, 0.0, 1.0);
    } else if (mode == 1) {
        a = texture(u_atlas, v_uv).r;
    } else if (mode == 2) {
        a = clamp(v_param.y + 0.5 - length(v_uv), 0.0, 1.0);
    } else if (mode == 3) {
        a = clamp(v_param.z * 0.5 + 0.5 - abs(length(v_uv) - v_param.y), 0.0, 1.0);
    } else {
        a = exp(-dot(v_uv, v_uv) / (v_param.y * v_param.y));
    }
    frag = vec4(v_color.rgb, v_color.a * a);
}
