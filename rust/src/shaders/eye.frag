#version 450
// The eye's fragment shader, carried over from renderer/eye.js.
// The body below main() is the WebGL shader unchanged except for three spots:
//   - uniforms live in one uniform block (wgpu has no loose uniforms),
//   - wgpu's gl_FragCoord starts at the top left, WebGL's at the bottom left,
//     so `fragXY` flips y back,
//   - the result goes to `outColor` instead of gl_FragColor.
// naga (wgpu's shader translator) compiles this GLSL for DX12 or Vulkan.

// Three vec4s keep the std140 layout simple; must match `Uniforms` in eye.rs.
layout(set = 0, binding = 0) uniform Params {
  vec4 a;   // res.x, res.y, flow, pupil
  vec4 b;   // gaze.x, gaze.y, center.x, center.y
  vec4 c;   // heat, flare, wide, dim
} P;

#define uRes    P.a.xy
#define uFlow   P.a.z
#define uPupil  P.a.w
#define uGaze   P.b.xy
#define uCenter P.b.zw
#define uHeat   P.c.x
#define uFlare  P.c.y
#define uWide   P.c.z
#define uDim    P.c.w
#define fragXY  vec2(gl_FragCoord.x, uRes.y - gl_FragCoord.y)

layout(location = 0) out vec4 outColor;

const float TAU = 6.2831853;
const float R = 0.2;    // sphere radius, in eye units

float hash(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
             mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}

float fbm(vec2 p) {
  float v = 0.0;
  float a = 0.5;
  for (int i = 0; i < 4; i++) {
    v += a * noise(p);
    p = p * 2.03 + vec2(1.7, 9.2);
    a *= 0.5;
  }
  return v;
}

// Noise that drifts along dir forever without its coordinates growing:
// two copies offset by a looping phase, crossfaded so the wrap never shows.
float flowNoise(vec2 p, vec2 dir) {
  float a = uFlow;
  float b = fract(uFlow + 0.5);
  float na = fbm(p + dir * a * 3.0);
  float nb = fbm(p + dir * b * 3.0 + vec2(31.7, 17.3));
  float wa = 1.0 - abs(2.0 * a - 1.0);
  return mix(nb, na, wa);
}

// Black body ramp: deep red, orange, yellow, white hot.
vec3 fire(float t) {
  t = max(t, 0.0);
  return vec3(min(1.0, t * 1.7), clamp(t * t * 1.25, 0.0, 1.0), clamp(pow(t, 4.0) * 0.9, 0.0, 1.0));
}

// One layer of rising sparks. Cells scroll up a whole number of cells per
// flow loop, and the hash repeats every 'period' rows, so the loop is seamless.
float embers(vec2 p, float scale, float cells, float period, float seed, float density) {
  vec2 q = p * scale + vec2(seed * 7.13, 0.0);
  q.y -= uFlow * cells;
  vec2 id = floor(q);
  vec2 f = fract(q);
  vec2 hid = vec2(id.x, mod(id.y, period)) + seed;
  float h = hash(hid);
  float h2 = hash(hid + 19.7);
  float on = clamp((density - h) * 6.0, 0.0, 1.0);
  vec2 c = vec2(0.25 + 0.5 * h2, 0.3 + 0.4 * h);
  c.x += 0.18 * sin(TAU * (uFlow * 2.0 + h * 3.0) + q.y * 1.3);
  vec2 d = (f - c) * vec2(1.0, 0.55);   // a little streaked upward
  float r = 0.07 + 0.06 * h2;
  float ld = length(d);
  float spark = smoothstep(r, 0.0, ld) + 0.3 * smoothstep(r * 2.6, 0.0, ld);
  float twinkle = 0.55 + 0.45 * sin(TAU * (uFlow * 3.0 + h2 * 5.0));
  return spark * on * twinkle;
}

void main() {
  float px = 1.0 / min(uRes.x, uRes.y);
  vec2 p = (fragXY - 0.5 * uRes) * px;
  p -= uCenter;
  float heat = clamp(uHeat + uFlare * 0.8, 0.0, 1.6);
  float wake = clamp(uWide, 0.0, 1.2);

  float rr = length(p);
  float r = rr / R;
  float dist = rr - R;           // distance outside the sphere, eye units

  // ---- Outside: glow and embers only. Nothing breaks the circle. ----
  vec2 dirv = p / max(rr, 1e-4);
  // A slow, radially symmetric breathing of the corona: noise sampled on a
  // circle that itself turns once per loop.
  vec2 spin = vec2(cos(TAU * uFlow), sin(TAU * uFlow));
  float breath = 0.8 + 0.4 * noise(dirv * 2.2 + spin * 0.9 + 4.0);
  float dOut = max(dist, 0.0);
  float halo = exp(-dOut * 24.0) * (0.30 + 0.35 * heat);
  float corona = exp(-dOut * 7.0) * (0.10 + 0.16 * heat) * breath;
  float glow = (halo + corona) * (0.35 + 0.65 * wake);
  vec3 glowCol = mix(vec3(0.75, 0.10, 0.01), vec3(1.0, 0.42, 0.08), exp(-dOut * 14.0));

  // Embers rise from the sphere: born near its surface, strongest above it,
  // fading as they climb. Their number follows the heat.
  float density = 0.10 + 0.45 * clamp(heat, 0.0, 1.0) + 0.3 * uFlare;
  float sp = embers(p, 22.0, 8.0, 8.0, 0.0, density)
           + embers(p, 34.0, 12.0, 12.0, 3.1, density) * 0.7
           + embers(p, 15.0, 6.0, 6.0, 7.7, density * 0.8) * 1.2;
  float above = smoothstep(-R * 0.6, R * 0.4, p.y);
  float band = smoothstep(0.004, 0.03, dist) * (1.0 - smoothstep(0.02, 0.10 + 0.14 * clamp(heat, 0.0, 1.0), dist));
  float width = 1.0 - smoothstep(R * 0.6, R * 1.25, abs(p.x));
  sp *= band * above * width * wake;
  vec3 spCol = fire(0.7 + 0.12 * min(heat, 1.0));

  vec3 outPm = glowCol * glow + spCol * sp;
  float outA = clamp(glow * 0.85 + sp, 0.0, 1.0);

  // ---- Inside: the sphere ----
  float z = sqrt(max(0.0, 1.0 - r * r));
  vec3 n = vec3(p / R, z);

  // The gaze turns the sphere: its pupil axis points along A.
  vec3 A = normalize(vec3(uGaze.x * 0.85, uGaze.y * 0.7, 1.0));
  vec3 rt = normalize(cross(vec3(0.0, 1.0, 0.0), A));
  vec3 up = cross(A, rt);
  vec3 l = vec3(dot(n, rt), dot(n, up), dot(n, A));   // surface point in eye space

  // Fire on the surface, streaming outward from the pupil.
  float s = length(l.xy);
  vec2 radial = l.xy / max(s, 1e-3);
  float swirl = flowNoise(l.xy * 3.2, -radial * 0.6);
  float fibers = noise(radial * 6.0 + 3.3) * 0.75 + noise(radial * 13.0 + 1.1) * 0.25;
  float front = smoothstep(-0.3, 1.0, l.z);
  float ang = atan(s, l.z) / 3.14159;                  // 0 at the pupil, 1 at the far pole
  float it = 0.92 - ang * 1.05
           + (swirl - 0.5) * 1.5 + (fibers - 0.5) * 0.4 * front
           + heat * 0.14;
  it *= 0.55 + 0.45 * wake;
  it = 1.06 * (1.0 - exp(-max(it, 0.0) * 1.9));       // soft cap: hot, not blown out
  it *= 0.55 + 0.45 * smoothstep(0.28, 0.62, swirl);  // dark veins in the fire
  it += exp(-ang * 7.0) * (0.32 + 0.2 * heat) * wake;  // the iris burns hottest round the pupil
  vec3 ecol = fire(it);

  // The slit pupil, painted on the sphere so it foreshortens near the limb.
  float pxl = px / (R * max(z, 0.08));                 // one pixel, in surface units
  float h = 0.78;
  float w = mix(0.025, 0.24, clamp(uPupil, 0.0, 1.0));
  float py = clamp(l.y / h, -1.0, 1.0);
  float slit = w * pow(max(0.0, 1.0 - py * py), 0.7);
  float pd = abs(l.x) - slit;
  float onFront = smoothstep(0.0, 0.12, l.z);
  float pupil = (1.0 - smoothstep(-pxl, pxl, pd)) * (1.0 - smoothstep(h - 0.06, h, abs(l.y))) * onFront;
  float ring = exp(-max(pd, 0.0) * 22.0) * (1.0 - pupil) * onFront * (1.0 - smoothstep(h * 0.7, h * 1.05, abs(l.y)));
  ecol += vec3(1.0, 0.8, 0.45) * ring * (0.35 + 0.45 * heat) * wake;
  vec3 pupilCol = mix(vec3(0.0), vec3(0.09, 0.008, 0.0), smoothstep(-0.05, 0.0, pd));
  ecol = mix(ecol, pupilCol, pupil);

  // Limb darkening, then a thin fresnel rim of hotter light at the edge.
  ecol *= mix(0.38, 1.0, sqrt(z));
  float fres = pow(1.0 - z, 4.0);
  ecol += vec3(1.0, 0.45, 0.1) * fres * (0.35 + 0.5 * heat) * (0.4 + 0.6 * wake);

  float inside = 1.0 - smoothstep(-px, px, dist);

  // Fade the light out before it reaches the window's edges.
  vec2 uv = fragXY / uRes;
  float border = smoothstep(0.0, 0.18, min(uv.x, 1.0 - uv.x)) * smoothstep(0.0, 0.18, min(uv.y, 1.0 - uv.y));
  border *= border;
  outPm *= border;
  outA *= border;

  vec3 pm = mix(outPm, ecol, inside);
  float alpha = mix(outA, 1.0, inside);

  float dimK = mix(1.0, 0.28, uDim);
  pm *= dimK;
  alpha *= mix(1.0, 0.75, uDim);
  outColor = vec4(pm, alpha);
}
