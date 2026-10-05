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

    vec3 background = vec3(0.039, 0.063, 0.094);  // ground #0a1018
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

    // Nocturne palette: day land is 2.4:1 against night land so the terminator reads at a glance.
    vec3 ocean_night = vec3(0.043, 0.075, 0.110);  // #0b131c
    vec3 ocean_day   = vec3(0.106, 0.184, 0.251);  // #1b2f40
    vec3 land_night  = vec3(0.082, 0.110, 0.141);  // #151c24
    vec3 land_day    = vec3(0.298, 0.349, 0.396);  // #4c5965
    vec3 coast_night = vec3(0.149, 0.192, 0.239);  // #26313d
    vec3 coast_day   = vec3(0.490, 0.545, 0.588);  // #7d8b96
    vec3 twilight    = vec3(0.420, 0.290, 0.227);  // #6b4a3a

    vec3 col = mix(mix(ocean_night, ocean_day, day), mix(land_night, land_day, day), land);
    col += shelf * 0.0105 * (0.4 + day) * vec3(0.5, 0.75, 1.0);
    col = mix(col, mix(coast_night, coast_day, day), coast * 0.55);
    col += dusk * twilight * 0.35 * (0.3 + 0.7 * land);

    // Graticule every 30°, barely visible.
    vec2 g = vec2(lon, lat) / (PI / 6.0);
    vec2 gd = abs(fract(g - 0.5) - 0.5) / fwidth(g);
    col = mix(col, vec3(0.933, 0.949, 0.957), (1.0 - min(min(gd.x, gd.y), 1.0)) * 0.025);

    // --- city lights, only where it is dark. A soft gamma keeps small towns; a coarse mip level
    // adds a halo around cities. Dim pixels are amber, bright ones white-gold.
    float l = texture(u_lights, tc).r;
    float halo = textureLod(u_lights, tc, 3.0).r;
    vec3 tint = mix(vec3(1.0, 0.604, 0.235), vec3(1.0, 0.890, 0.690), min(l * 1.3, 1.0)); // #ff9a3c -> #ffe3b0
    col += tint * (pow(l, 0.75) * 1.05 + halo * 0.9) * night;

    // Vignette + dither.
    vec2 q = px / u_res - 0.5;
    col *= 1.0 - dot(q, q) * 0.45;
    col += (hash(px) - 0.5) / 255.0;
    frag = vec4(col, 1.0);
}
