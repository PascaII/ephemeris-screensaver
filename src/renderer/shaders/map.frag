#version 330 core
// World map: Miller projection, land SDF, day/night with twilight, NASA night lights.

uniform vec2 u_res;          // framebuffer size in pixels
uniform vec4 u_view;         // x: y_top (Miller units), y: y_span, z: centre longitude (rad), w: x offset px
uniform vec2 u_sun;          // subsolar point (lat, lon) in radians
uniform sampler2D u_land;    // R8 SDF, 128 = coast, 8 steps per texel, land positive
uniform sampler2D u_lights;  // R8 night lights

out vec4 frag;

const float PI = 3.14159265359;

// Inverse Miller cylindrical projection.
float miller_lat(float y) { return (atan(exp(y / 1.25)) - PI / 4.0) / 0.4; }

vec3 unit(float lat, float lon) { return vec3(cos(lat) * cos(lon), cos(lat) * sin(lon), sin(lat)); }

// Cheap hash for dithering (prevents banding in the very dark gradients).
float hash(vec2 p) { return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453); }

void main() {
    vec2 px = gl_FragCoord.xy;
    float map_w = u_res.x - 2.0 * u_view.w;
    float lon = ((px.x - u_view.w) / map_w - 0.5) * 2.0 * PI + u_view.z;
    float lat = miller_lat(u_view.x - (1.0 - px.y / u_res.y) * u_view.y);

    vec3 background = vec3(0.012, 0.016, 0.022);
    if (abs(lat) > 1.55 || px.x < u_view.w || px.x > u_res.x - u_view.w) {
        frag = vec4(background, 1.0);
        return;
    }

    vec2 tc = vec2(fract(lon / (2.0 * PI) + 0.5), 0.5 - lat / PI);

    // --- land / coast from the signed distance field
    float sdf = (texture(u_land, tc).r * 255.0 - 128.0) / 8.0; // in SDF texels
    float aa = max(fwidth(sdf), 1e-4) * 0.75;
    float land = smoothstep(-aa, aa, sdf);
    float coast = 1.0 - smoothstep(0.0, aa * 1.6, abs(sdf));
    float shelf = exp(-max(-sdf, 0.0) * 0.35) * (1.0 - land); // faint glow off the coast

    // --- sun
    float cosz = dot(unit(lat, lon), unit(u_sun.x, u_sun.y)); // sine of solar altitude
    float day = smoothstep(-0.035, 0.10, cosz);                // fully lit a little above horizon
    float night = 1.0 - smoothstep(-0.21, -0.02, cosz);        // fully dark below ~ -12°
    float dusk = exp(-pow((cosz + 0.05) / 0.09, 2.0));         // soft band just past the terminator

    vec3 ocean_night = vec3(0.016, 0.022, 0.032);
    vec3 ocean_day   = vec3(0.040, 0.066, 0.096);
    vec3 land_night  = vec3(0.040, 0.048, 0.060);
    vec3 land_day    = vec3(0.135, 0.150, 0.165);

    vec3 ocean = mix(ocean_night, ocean_day, day) + shelf * mix(0.010, 0.022, day) * vec3(0.5, 0.75, 1.0);
    vec3 col = mix(ocean, mix(land_night, land_day, day), land);
    col += coast * mix(vec3(0.05, 0.065, 0.08), vec3(0.11, 0.13, 0.15), day);
    col += dusk * vec3(0.040, 0.024, 0.034) * (0.3 + 0.7 * land);

    // Graticule every 30°, barely visible.
    vec2 g = vec2(lon, lat) / (PI / 6.0);
    vec2 gd = abs(fract(g - 0.5) - 0.5) / fwidth(g);
    col += (1.0 - min(min(gd.x, gd.y), 1.0)) * 0.012;

    // --- city lights, only where it is dark
    float l = texture(u_lights, tc).r;
    vec3 amber = vec3(1.0, 0.72, 0.38);
    col += amber * pow(l, 1.6) * 1.35 * night;

    // Vignette + dither.
    vec2 q = px / u_res - 0.5;
    col *= 1.0 - dot(q, q) * 0.55;
    col += (hash(px) - 0.5) / 255.0;
    frag = vec4(col, 1.0);
}
