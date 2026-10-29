// ---- The aura: everything outside the sphere. Only light. ----
// Returns premultiplied colour and alpha. It is drawn under the body, so it
// may glow right up to (and behind) the silhouette, but it must never
// change the sphere's outline: everything here is soft light or tiny
// antialiased sparks, nothing with a hard edge.
//
// Layers, back to front:
//   1. corona   wide, faint, fades well before the window edge
//   2. halo     mid glow, stronger above the ball as it heats up
//   3. tongues  soft flame-shaped light streaming outward and bending up,
//               periodic noise in polar coordinates (seamless round the
//               circle and across the flow loop)
//   4. shimmer  heat haze: the glow's distance wavers just above the ball
//   5. rim      a tight hot bloom hugging the silhouette
//   6. wince    a soft ring of light that rushes outward on a flinch
//   7. embers   three depth layers rising on a swirling, fanning plume,
//               cooling from hot to deep as they climb
//   8. sparks   rare, fast, white-hot streaks
// Everything that moves loops with uFlow, so nothing ever jumps.

// Value noise whose lattice wraps every 'per' cells, so scrolling it by a
// whole number of periods per flow loop is seamless.
float auraPNoise(vec2 p, vec2 per) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  vec2 i0 = mod(i, per);
  vec2 i1 = mod(i + 1.0, per);
  return mix(mix(hash(i0), hash(vec2(i1.x, i0.y)), u.x),
             mix(hash(vec2(i0.x, i1.y)), hash(i1), u.x), u.y);
}

// One layer of embers. Cells scroll up 'cells' rows per flow loop and the
// hash repeats every 'cells' rows, so the loop is seamless. Returns the
// premultiplied colour in rgb and the coverage in a.
//   pe    position, already bent by the plume and the swirl
//   dist  true distance outside the sphere (for life and cooling)
//   pxs   one pixel in this layer's cell units
//   life  how far (eye units) the longest-lived embers climb
//   tail  streak length behind each ember
//   hot   fire() temperature at birth
vec4 auraEmbers(vec2 pe, float scale, float cells, float seed, float density,
                float pxs, float dist, float life, float tail, float hot) {
  vec2 q = pe * scale + vec2(seed * 7.13, seed * 3.71);
  q.y -= uFlow * cells;
  vec2 id = floor(q);
  vec2 f = fract(q);
  vec2 hid = vec2(mod(id.x, 97.0), mod(id.y, cells)) + seed;
  float h1 = hash(hid);
  float on = clamp((density - h1) * 10.0, 0.0, 1.0);
  if (on <= 0.0) return vec4(0.0);
  float h2 = hash(hid + 19.7);
  float h3 = hash(hid + 41.3);

  // Each ember wanders a little side to side on its own clock.
  vec2 c = vec2(0.4 + 0.2 * h2, 0.6 + 0.1 * h3);
  c.x += 0.05 * sin(TAU * (uFlow * 2.0 + h3));
  vec2 d = f - c;
  // A short streak trailing below the head (they rise).
  d.y /= d.y < 0.0 ? 1.0 + tail * (0.5 + h2) : 1.0;

  // Antialiased: never smaller than a pixel; smaller ones get dimmer
  // instead of shimmering, so the energy stays the same.
  float r = mix(0.045, 0.08, h2 * h2);
  float s = max(r, pxs * 0.7);
  float e2 = dot(d, d);
  float core = exp(-e2 / (s * s)) * (r * r) / (s * s);
  float glow = exp(-e2 / (s * s * 3.3)) * 0.2;

  // Each ember lives a different height; it cools and fades as it climbs.
  float L = life * (0.55 + 0.45 * h3);
  float age = clamp(dist / L, 0.0, 1.0);
  float fade = (1.0 - smoothstep(0.45, 1.0, age)) * (1.0 - 0.25 * age);
  float twinkle = 0.7 + 0.3 * sin(TAU * (uFlow * 3.0 + h2 * 7.0));
  float k = on * fade * twinkle;
  vec3 col = fire(mix(hot, 0.62, age));
  return vec4(col * (core + glow) * k, min((core * 0.95 + glow * 0.4) * k, 1.0));
}

vec4 aura(vec2 p, float px, float heat, float wake) {
  float rr = length(p);
  float dist = rr - R;            // distance outside the sphere, eye units
  if (dist > 0.33) return vec4(0.0);   // every layer is below 1/255 out here
  float dOut = max(dist, 0.0);
  vec2 dirv = p / max(rr, 1e-4);
  float h01 = clamp(heat, 0.0, 1.0);
  float gk = max(uGlowK, 0.05);

  // Where the heat goes: up, leaning away from the way the eye is turning.
  vec2 gv = clamp(uGazeV, vec2(-3.0), vec2(3.0));
  vec2 upDir = normalize(vec2(0.0, 1.0) - gv * 0.18);
  float up = dot(dirv, upDir);                 // -1 below .. 1 above
  float above = smoothstep(-0.2, 0.7, up);

  // ---- 4. Heat shimmer: the glow's distance wavers above the ball. ----
  float sh = auraPNoise(vec2(p.x * 38.0, p.y * 38.0 - uFlow * 32.0), vec2(64.0, 16.0)) - 0.5;
  float dGlow = dOut + sh * 0.012 * above * (0.4 + h01) * smoothstep(0.0, 0.02, dOut);
  // Heat stretches the glow upward: distance counts less above the ball.
  float lift = 1.0 + (0.35 + 0.9 * h01) * max(up, 0.0);
  float dUp = dGlow / lift;

  // ---- 3. Tongues: soft flame light, polar, periodic in angle and flow. ----
  vec2 bent = normalize(dirv + upDir * dOut * (3.0 + 4.0 * h01));
  float ang = atan(bent.y, bent.x) / TAU + 0.5 + dOut * 0.9;   // a slight spiral
  float rad = dOut * 16.0 - uFlow * 3.0;                        // streams outward
  float t1 = auraPNoise(vec2(ang * 9.0, rad), vec2(9.0, 3.0));
  float t2 = auraPNoise(vec2(ang * 18.0 + 3.0, rad * 2.0 + 5.0), vec2(18.0, 6.0));
  float tongues = t1 * 0.65 + t2 * 0.35;
  // Even near the ball, sharpening outward into tongues; strongest above.
  float tgK = smoothstep(0.0, 0.05, dOut) * (0.45 + 0.55 * above);
  float tg = mix(1.0, smoothstep(0.25, 0.8, tongues) * 1.9, tgK);

  // A slow drift of the whole glow, and its swell on the inhale.
  vec2 spin = vec2(cos(TAU * uFlow), sin(TAU * uFlow));
  float drift = 0.85 + 0.3 * noise(dirv * 1.6 + spin * 0.7 + 4.0);
  float breathK = 1.0 + 0.12 * uBreath;
  float wince = clamp(uWince, 0.0, 1.0);
  float wakeK = (0.35 + 0.65 * wake) * min(gk, 1.25);

  // ---- 1, 2, 5. Corona, halo and rim. ----
  float reach = gk * breathK;
  float corona = exp(-dUp * 7.5 / reach) * (0.12 + 0.14 * heat) * (0.5 + 0.7 * tg) * drift;
  corona *= 1.0 - smoothstep(0.10, 0.27, dist);               // gone before the window edge
  float halo = exp(-dUp * 20.0 / reach) * (0.40 + 0.38 * heat) * (0.65 + 0.5 * tg) * drift;
  float rim = exp(-dOut * 85.0) * (0.6 + 0.4 * heat) * (0.9 + 0.2 * uBreath);

  // ---- 6. Wince: a soft ring of light rushing outward, and a flash. ----
  float ringR = (1.0 - wince) * 0.16;
  float ringW = 0.02 + 0.04 * (1.0 - wince);
  float ring = exp(-(dOut - ringR) * (dOut - ringR) / (ringW * ringW)) * wince * wince * 0.45;
  float flash = 1.0 + 1.2 * wince;

  // Colour: white-hot at the skin, the mood's glow, then deep fire outside.
  vec3 hotCol = mix(uCG, fire(1.05), 0.55);
  vec3 midCol = mix(uCG, fire(0.75), 0.25);
  vec3 farCol = mix(uCG * 0.8, uC0 + uC1 * 0.3, 0.45);
  vec3 glowRGB = hotCol * (rim + ring) + midCol * halo + farCol * corona;
  float glowA = rim * 0.6 + ring * 0.5 + halo * 0.55 + corona * 0.45;
  glowRGB *= wakeK * flash;
  glowA *= wakeK * flash;

  // ---- 7, 8. Embers and sparks. ----
  vec4 em = vec4(0.0);
  float plume = above * wake;
  if (plume > 0.001 && dist > -0.02 && dist < 0.30) {
    // The plume: embers fan outward as they climb and trail opposite to
    // the gaze's motion, more the farther they are from the ball.
    vec2 pe = p;
    pe += gv * 0.10 * dOut;
    pe.x /= 1.0 + 1.0 * max(pe.y, 0.0);
    // The swirl: a slow, rising, looping field bends their paths.
    float sw = auraPNoise(vec2(pe.x * 7.0 + 11.0, pe.y * 7.0 - uFlow * 4.0), vec2(31.0, 4.0)) - 0.5;
    float sw2 = auraPNoise(vec2(pe.x * 7.0 + 3.0, pe.y * 7.0 - uFlow * 4.0 + 2.0), vec2(31.0, 4.0)) - 0.5;
    pe += vec2(sw, sw2 * 0.4) * (0.03 + 0.25 * dOut);

    float flare = clamp(uFlare, 0.0, 1.5);
    float density = (0.22 + 0.4 * h01 + 0.25 * flare + 0.3 * wince) * uEmberK;
    float life = 0.17 + 0.10 * h01 + 0.05 * flare;
    float hot = 1.0 + 0.15 * h01;
    em += auraEmbers(pe, 18.0, 8.0, 0.0, density, px * 18.0, dist, life, 1.2, hot);                           // mid
    em += auraEmbers(pe, 27.0, 10.0, 3.1, density * 1.2, px * 27.0, dist, life * 0.8, 0.8, hot - 0.1) * 0.6;   // far: small, slow
    em += auraEmbers(pe, 12.0, 5.0, 7.7, density * 0.8, px * 12.0, dist, life * 1.15, 1.4, hot + 0.05);        // near: big
    // Rare white-hot sparks: fast and streaked.
    float sparkD = (0.03 + 0.15 * h01 + 0.4 * flare + 0.6 * wince) * uEmberK;
    em += auraEmbers(pe, 16.0, 12.0, 12.9, sparkD, px * 16.0, dist, life * 1.4, 2.0, 1.45) * 1.3;
    em *= plume;
  }

  vec3 rgb = glowRGB + em.rgb;
  float a = clamp(glowA + em.a, 0.0, 1.0);

  // A whisper of dither so the long soft gradients never band.
  float dz = (hash(fragXY) - 0.5) / 255.0;
  float dk = min(a * 6.0, 1.0);
  rgb = max(rgb + dz * dk, 0.0);
  a = clamp(a + dz * dk, 0.0, 1.0);
  return vec4(rgb, a);
}
