#version 450
// The Burning Eye, in layers. The shader is four files pasted together in
// this order (see `eye::shader_source`):
//   common.glsl  uniforms, noise and the mood's fire ramp (this file),
//   body.glsl    the sphere and everything inside it: the fire, the pupil,
//   aura.glsl    everything outside it: only light, glows and embers,
//   eye.frag     main(), which lays the aura under the body.
// Art rules: a perfect sphere is the resting state. Effects live inside it;
// only glows and particles go outside, and nothing breaks the circle.
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
  vec4 d;   // fade, size, time, breath
  vec4 e;   // mood colour 0 (deep fire), swirl
  vec4 f;   // mood colour 1 (flame), glow strength
  vec4 g;   // mood colour 2 (hot core), ember amount
  vec4 h;   // mood colour 3 (glow), morph
  vec4 i;   // gaze velocity.x, gaze velocity.y, pupil velocity, quality
  vec4 j;   // wince, roll, unused, unused
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
#define uTime   P.d.z   // seconds since start, wrapped every hour; for things that needn't loop
#define uBreath P.d.w   // -1..1, a slow breath: inhale > 0
#define uMorph  P.h.w   // 0 = the perfect resting sphere; > 0 lets the body deform
#define uGazeV  P.i.xy  // how fast the gaze is turning, gaze units per second
#define uPupilV P.i.z   // how fast the pupil is opening (> 0) or closing
#define uQual   P.i.w   // 0..1, how many steps the raymarches can afford
#define uWince  P.j.x   // 0..1, a flinch: the body tightens and the fire recoils
#define uRoll   P.j.y   // radians, the sphere's slow roll about the view axis
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
