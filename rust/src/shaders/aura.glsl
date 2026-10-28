// ---- The aura: everything outside the sphere. Only light. ----
// Returns premultiplied colour and alpha. It is drawn under the body, so it
// may glow right up to (and behind) the silhouette, but it must never
// change the sphere's outline.

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

vec4 aura(vec2 p, float px, float heat, float wake) {
  float rr = length(p);
  float dist = rr - R;           // distance outside the sphere, eye units

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

  vec3 pm = glowCol * glow + spCol * sp;
  float a = clamp(glow * 0.85 + sp, 0.0, 1.0);
  return vec4(pm, a);
}
