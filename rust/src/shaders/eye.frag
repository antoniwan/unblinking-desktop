#version 450
// The Burning Eye: the "v2 wrapped" fire from the Blinki Studio.
// The fire lives IN the sphere: noise is sampled in 3D on its surface, so it
// wraps and foreshortens with the ball, and a hotter inner layer shows
// through the dark veins for depth. The pupil is solid: dark all the way
// through, with a soft inky edge and a thin hot lip where the fire meets it.
// Outside the circle there is only light: a soft glow and rising embers.
//
// Differences from the studio's WebGL source:
//   - uniforms live in one uniform block (wgpu has no loose uniforms),
//   - wgpu's gl_FragCoord starts at the top left, WebGL's at the bottom
//     left, so `fragXY` flips y back,
//   - uFade and uSize let the app ease the whole eye in and out,
//   - the result goes to `outColor` instead of gl_FragColor.
// naga (wgpu's shader translator) compiles this GLSL for DX12 or Vulkan.

// vec4s keep the std140 layout simple; must match `Uniforms` in eye.rs.
layout(set = 0, binding = 0) uniform Params {
  vec4 a;   // res.x, res.y, flow, pupil
  vec4 b;   // gaze.x, gaze.y, center.x, center.y
  vec4 c;   // heat, flare, wide, dim
  vec4 d;   // fade, size, unused, unused
  vec4 e;   // mood colour 0 (deep fire), swirl
  vec4 f;   // mood colour 1 (flame), glow strength
  vec4 g;   // mood colour 2 (hot core), ember amount
  vec4 h;   // mood colour 3 (glow), unused
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
#define uFade   P.d.x   // whole-eye opacity, for opening and closing
#define uSize   P.d.y   // whole-eye scale, for opening and closing
#define uC0     P.e.rgb
#define uSwirl  P.e.w
#define uC1     P.f.rgb
#define uGlowK  P.f.w
#define uC2     P.g.rgb
#define uEmberK P.g.w
#define uCG     P.h.rgb
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

float hash3(vec3 p) {
  p = fract(p * 0.3183099 + 0.1);
  p *= 17.0;
  return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}

float noise3(vec3 x) {
  vec3 i = floor(x);
  vec3 f = fract(x);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(mix(hash3(i), hash3(i + vec3(1.0, 0.0, 0.0)), f.x),
                 mix(hash3(i + vec3(0.0, 1.0, 0.0)), hash3(i + vec3(1.0, 1.0, 0.0)), f.x), f.y),
             mix(mix(hash3(i + vec3(0.0, 0.0, 1.0)), hash3(i + vec3(1.0, 0.0, 1.0)), f.x),
                 mix(hash3(i + vec3(0.0, 1.0, 1.0)), hash3(i + vec3(1.0, 1.0, 1.0)), f.x), f.y), f.z);
}

float fbm3(vec3 p) {
  float v = 0.0;
  float a = 0.5;
  for (int i = 0; i < 4; i++) {
    v += a * noise3(p);
    p = p * 2.02 + vec3(1.7, 9.2, 3.1);
    a *= 0.5;
  }
  return v;
}

// Noise that drifts along dir forever without its coordinates growing:
// two copies offset by a looping phase, crossfaded so the wrap never shows.
float flow3(vec3 p, vec3 dir) {
  float a = uFlow;
  float b = fract(uFlow + 0.5);
  float na = fbm3(p + dir * a * 2.0);
  float nb = fbm3(p + dir * b * 2.0 + vec3(31.7, 17.3, 5.1));
  float wa = 1.0 - abs(2.0 * a - 1.0);
  return mix(nb, na, wa);
}

// The mood's palette in place of a fixed black-body ramp:
// deep, flame, hot core, then a white tip.
vec3 fire(float t) {
  t = max(t, 0.0);
  vec3 c = mix(vec3(0.0), uC0, smoothstep(0.0, 0.35, t));
  c = mix(c, uC1, smoothstep(0.3, 0.7, t));
  c = mix(c, uC2, smoothstep(0.65, 1.0, t));
  return mix(c, vec3(1.0, 0.98, 0.94), smoothstep(1.05, 1.5, t));
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
  // Grow from small while opening, shrink away while closing.
  p /= uSize;
  px /= uSize;
  float heat = clamp(uHeat + uFlare * 0.8, 0.0, 1.8);
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
  float gk = max(uGlowK, 0.05);
  float halo = exp(-dOut * 24.0 / gk) * (0.30 + 0.35 * heat);
  float corona = exp(-dOut * 7.0 / gk) * (0.10 + 0.16 * heat) * breath;
  float glow = (halo + corona) * (0.35 + 0.65 * wake) * min(gk, 1.0);
  vec3 glowCol = mix(uCG * 0.75, uCG, exp(-dOut * 14.0));

  // Embers rise from the sphere: born near its surface, strongest above it,
  // fading as they climb. Their number follows the heat and the mood.
  float density = (0.10 + 0.45 * clamp(heat, 0.0, 1.0) + 0.3 * uFlare) * uEmberK;
  float sp = embers(p, 22.0, 8.0, 8.0, 0.0, density)
           + embers(p, 34.0, 12.0, 12.0, 3.1, density) * 0.7
           + embers(p, 15.0, 6.0, 6.0, 7.7, density * 0.8) * 1.2;
  float above = smoothstep(-R * 0.6, R * 0.4, p.y);
  float band = smoothstep(0.004, 0.03, dist) * (1.0 - smoothstep(0.02, 0.10 + 0.14 * clamp(heat, 0.0, 1.0), dist));
  float width = 1.0 - smoothstep(R * 0.6, R * 1.25, abs(p.x));
  sp *= band * above * width * wake;
  vec3 spCol = fire(0.85 + 0.12 * min(heat, 1.0));

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

  // A second, smaller sphere inside: the deeper, hotter layer.
  float k = 0.88;
  float r2 = dot(p / R, p / R);
  vec3 nIn = r2 < k * k ? vec3(p / R, sqrt(k * k - r2)) / k : n;
  vec3 li = vec3(dot(nIn, rt), dot(nIn, up), dot(nIn, A));
  float ang = acos(clamp(l.z, -1.0, 1.0)) / 3.14159;      // 0 at the pupil, 1 at the far pole
  float angIn = acos(clamp(li.z, -1.0, 1.0)) / 3.14159;
  float outer = flow3(l * uSwirl * 1.1, vec3(0.0, 0.0, 1.0));
  float inner = flow3(li * uSwirl * 0.7 + vec3(11.0), vec3(0.0, 0.0, 0.7));
  vec2 radial = l.xy / max(length(l.xy), 1e-3);
  float fibers = noise(radial * 7.0 + 3.3) * 0.7 + noise(radial * 15.0 + 1.1) * 0.3;
  float itO = 0.88 - ang * 1.1 + (outer - 0.5) * 1.4 + (fibers - 0.5) * 0.6 * smoothstep(-0.3, 1.0, l.z) + heat * 0.14;
  float itI = 0.95 - angIn * 1.0 + (inner - 0.5) * 1.0 + heat * 0.18;
  float veil = smoothstep(0.32, 0.62, outer);              // where the outer flames thin, see the inner
  float it = mix(itI * 0.7, itO, veil);
  it *= 0.55 + 0.45 * wake;
  it = 1.06 * (1.0 - exp(-max(it, 0.0) * 1.9));            // soft cap: hot, not blown out
  it *= 0.45 + 0.55 * smoothstep(0.25, 0.6, max(outer, inner * 0.8));
  it += exp(-ang * 5.0) * (0.38 + 0.22 * heat) * wake;     // hottest round the pupil
  vec3 ecol = fire(it);

  // The solid pupil, painted on the surface so it foreshortens with the ball.
  float pxl = px / (R * max(z, 0.08));                     // one pixel, in surface units
  float h = 0.78;
  float w = mix(0.025, 0.24, clamp(uPupil, 0.0, 1.0));
  float onFront = smoothstep(0.0, 0.12, l.z);
  float py = clamp(l.y / h, -1.0, 1.0);
  float slit = w * pow(max(0.0, 1.0 - py * py), 0.7);
  float pd = abs(l.x) - slit;
  float vmask = (1.0 - smoothstep(h - 0.08, h, abs(l.y))) * onFront;
  float pupil = (1.0 - smoothstep(-pxl, pxl, pd)) * vmask;
  float ink = (1.0 - smoothstep(0.0, 0.05 + 0.03 * w, pd)) * vmask * (1.0 - pupil);
  float lip = exp(-max(pd, 0.0) * 26.0) * (1.0 - pupil) * vmask;
  ecol *= 1.0 - 0.55 * ink;
  ecol += fire(1.15) * lip * (0.22 + 0.35 * heat) * wake;
  vec3 pupilCol = mix(vec3(0.0), uC0 * 0.08, smoothstep(-slit * 0.9, 0.0, pd));
  ecol = mix(ecol, pupilCol, pupil);

  // Limb darkening, then a thin fresnel rim of the mood's glow at the edge.
  ecol *= mix(0.38, 1.0, sqrt(z));
  float fres = pow(1.0 - z, 4.0);
  ecol += uCG * fres * (0.35 + 0.5 * heat) * (0.4 + 0.6 * wake);

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
  // Premultiplied, so fading is one multiply of colour and alpha together.
  outColor = vec4(pm, alpha) * uFade;
}
