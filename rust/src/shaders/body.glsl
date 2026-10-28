// ---- The body: the sphere and everything inside it. ----
// Returns premultiplied colour, with alpha = how much of this pixel the
// sphere covers. Nothing here may draw outside the sphere's silhouette.
//
// The body is a signed distance field: a unit sphere (scaled by R), plus a
// smooth deformation that is exactly zero while uMorph is 0. At rest the
// ray/sphere hits are solved analytically; only a morphing body is
// sphere-traced. Inside, the ray from the front surface to the back one
// marches a volume of emissive fire, front to back, in layers:
//   1. a domain warp: two low noises boiling on a loop,
//   2. the boil: big warped blobs drifting out along the pupil axis, plus
//      a finer octave boiling in place (noise of warped noise),
//   3. flame tongues: noise in cylindrical coordinates round the pupil
//      axis, stretched and scrolled outward, their ridges the bright licks,
//   4. a hot core round the centre (it breathes) and heat along the axis,
//   5. smoky threads that absorb what is behind them,
//   6. a thin skin of low flame that lights the limb,
//   and the slit: a slab of the volume left empty behind the pupil, so the
//   dark cut has depth when the ball turns.
// Turning the gaze turns this whole volume, so its layers part in parallax.
// The surface then adds the solid pupil, its inky edge and hot lip, limb
// darkening, a fresnel rim of the mood's glow and a faint moving sheen.
// All work happens in unit-sphere coordinates (q = p / R).

// Rotate a 2D vector by angle a.
vec2 bRot(vec2 v, float a) {
  float c = cos(a);
  float s = sin(a);
  return vec2(c * v.x - s * v.y, s * v.x + c * v.y);
}

// The pupil's half-width at height y on the unit sphere, in the eye's frame.
const float SLIT_H = 0.78;
float slitHalf(float y, float w) {
  float py = clamp(y / SLIT_H, -1.0, 1.0);
  return w * pow(max(0.0, 1.0 - py * py), 0.7);
}

// The deformation, scaled by uMorph: the surface's radius in direction d
// (a unit vector) is 1 + morph * bDisp(d), a slow noise swell plus a squash
// along the gaze. |bDisp| <= 0.13, so the body stays inside 1 + 0.13 morph.
// Being a function of direction only, the body stays star-shaped, and its
// silhouette is (nearly) the radius on the view plane: smooth to antialias.
float bDisp(vec3 d, vec3 A) {
  float drift = TAU * uTime / 3600.0 * 9.0;   // whole turns per hour: no seam at the wrap
  vec3 o = vec3(cos(drift), sin(drift), 0.0) * 0.8;
  float n = noise3(d * 1.6 + o + vec3(3.1, 7.7, 1.3)) * 2.0 - 1.0;
  float ax = dot(d, A);
  return 0.08 * n - 0.05 * (ax * ax - 0.33);
}

float bSdf(vec3 x, vec3 A, float m) {
  float l = length(x);
  return (l - 1.0 - m * bDisp(x / max(l, 1e-4), A)) * 0.9;
}

vec4 body(vec2 p, float px, float heat, float wake) {
  vec2 q = p / R;               // unit-sphere coordinates
  float pq = px / R;            // one pixel, in those units
  float r2 = dot(q, q);
  float m = clamp(uMorph, 0.0, 1.0);
  float Rb = 1.0 + 0.13 * m;    // bounding sphere of the morphed body
  if (r2 > (Rb + 2.0 * pq) * (Rb + 2.0 * pq)) {
    return vec4(0.0);
  }

  // The gaze turns the sphere: its pupil axis points along A.
  vec3 A = normalize(vec3(uGaze.x * 0.85, uGaze.y * 0.7, 1.0));
  vec3 rt = normalize(cross(vec3(0.0, 1.0, 0.0), A));
  vec3 up = cross(A, rt);

  // ---- Where the view ray enters and leaves the body. ----
  float zf;       // front surface, view z (towards the viewer)
  float zb;       // back surface
  vec3 n;         // front surface normal
  vec3 sp;        // front surface point's direction from the centre
  float inside;   // coverage
  if (m < 1e-4) {
    float z = sqrt(max(0.0, 1.0 - r2));
    zf = z;
    zb = -z;
    n = vec3(q, z);
    sp = n;
    inside = 1.0 - smoothstep(-pq, pq, sqrt(r2) - 1.0);
  } else {
    float zb0 = sqrt(max(0.0, Rb * Rb - r2));
    // The silhouette: the surface's radius on the view plane.
    float rEdge = 1.0 + m * bDisp(vec3(q / max(sqrt(r2), 1e-4), 0.0), A);
    inside = 1.0 - smoothstep(-pq, pq, sqrt(r2) - rEdge);
    // Front: sphere-trace down from the bounding sphere; back: up from
    // the far side. A grazing ray that never converges falls back to the
    // view plane, where the volume is empty and only the rim shows.
    float z = zb0;
    bool hit = false;
    for (int i = 0; i < 24; i++) {
      float d = bSdf(vec3(q, z), A, m);
      if (d < 5e-4) { hit = true; break; }
      if (z < 0.0) { break; }
      z -= d;
    }
    zf = hit ? z : 0.0;
    z = -zb0;
    for (int i = 0; i < 24; i++) {
      float d = bSdf(vec3(q, z), A, m);
      if (d < 5e-4 || z > zf) { break; }
      z += d;
    }
    zb = min(z, zf);
    vec3 xf = vec3(q, zf);
    vec2 h = vec2(0.01, 0.0);
    n = normalize(vec3(bSdf(xf + h.xyy, A, m) - bSdf(xf - h.xyy, A, m),
                       bSdf(xf + h.yxy, A, m) - bSdf(xf - h.yxy, A, m),
                       bSdf(xf + h.yyx, A, m) - bSdf(xf - h.yyx, A, m)));
    sp = normalize(xf);
  }
  float nz = clamp(n.z, 0.0, 1.0);

  // ---- The moment's motion, as offsets and scales of the fire. ----
  float breath = clamp(uBreath, -1.0, 1.0);
  float wince = clamp(uWince, 0.0, 1.0);
  float th = TAU * uFlow;                              // the looping phase
  vec3 circ = vec3(cos(th), sin(th), 0.0);             // a circle in noise space: loops
  float slow = TAU * uTime / 3600.0 * 5.0;             // a slow drift that never repeats soon
  float roll = uRoll + 0.12 * sin(th) + 0.05 * sin(slow);
  // The fire lags behind a turn: it is sampled ahead of the motion.
  vec2 gv = uGazeV * vec2(0.85, 0.7) * 0.09;
  float gl = length(gv);
  gv *= min(gl, 0.32) / max(gl, 1e-4);
  float sw = uSwirl;
  float pupilW = mix(0.025, 0.24, clamp(uPupil, 0.0, 1.0)) * (1.0 - 0.35 * wince);
  float pv = clamp(uPupilV, -3.0, 3.0);
  float recoil = 1.0 + 0.45 * wince - 0.04 * pv;       // > 1 pulls the fire towards the core
  float coreR = 0.42 * (1.0 + 0.08 * breath) * (1.0 - 0.25 * wince);

  // ---- The volume: front to back through the ball. ----
  int steps = int(mix(14.0, 32.0, clamp(uQual, 0.0, 1.0)));
  float L = max(zf - zb, 0.0);
  float invN = 1.0 / float(steps);
  // Steps bunch up near the front (u^1.6), where the eye looks hardest.
  // A per-pixel hash dithers the start: kills step banding, and being fixed
  // in screen space it never flickers.
  float dith = 0.5 + 0.6 * (hash(fragXY * 0.731 + 17.3) - 0.5);
  vec3 col = vec3(0.0);
  float trans = 1.0;
  for (int i = 0; i < 32; i++) {
    if (i >= steps || trans < 0.03) { break; }
    float u = (float(i) + dith) * invN;
    float su = pow(u, 0.6);
    float dt = L * 1.6 * su * invN;                    // d(u^1.6)/du * du
    vec3 X = vec3(q, zf - L * u * su);
    vec3 e = vec3(dot(X, rt), dot(X, up), dot(X, A)); // the eye's frame: z is the pupil axis
    float s = length(e);
    float depth = 1.0 - clamp(s, 0.0, 1.0);            // 0 at the skin, 1 at the centre

    // The slit, cut through the front of the volume.
    float sh = slitHalf(e.y, pupilW);
    // Prefiltered: the edge is widened by how far e.x moves in one step,
    // so a ray grazing the slab never aliases into grain.
    float fw = abs(rt.z) * dt;
    float cut = smoothstep(sh * 0.6 - fw, sh * 1.4 + 0.05 + fw, abs(e.x));
    cut = mix(1.0, cut, smoothstep(0.2, 0.6, e.z) * (1.0 - smoothstep(SLIT_H - 0.1, SLIT_H + 0.05, abs(e.y))));

    // Fire coordinates: rolled, twisted (more deeply inside), lagging.
    vec3 f = e * recoil;
    f.xy = bRot(f.xy, roll + 1.3 * depth * depth + 0.25 * sin(th + 2.0 * depth));
    f.xy += gv * (0.3 + 0.7 * depth);

    // 1. Domain warp: low noise, boiling on a loop.
    vec3 wp = f * (sw * 0.45) + circ * 0.55;
    float w1 = noise3(wp);
    float w2 = noise3(wp.yzx + vec3(5.2, 1.3, 7.9) - circ.yxz * 0.35);
    vec3 warp = vec3(w1 - 0.5, w2 - 0.5, (w1 - w2) * 0.7) * 1.1;

    // 2. The boil: big warped blobs, drifting out along the pupil axis;
    //    two copies crossfaded so the loop never shows.
    float pa = uFlow;
    float pb = fract(uFlow + 0.5);
    float wa = 1.0 - abs(2.0 * pa - 1.0);
    vec3 fp = (f + warp * 0.6) * sw * 0.75;
    vec3 dir = vec3(0.0, 0.3, -1.0) * 1.4;
    float F = mix(noise3(fp + dir * pb + vec3(31.7, 17.3, 5.1)), noise3(fp + dir * pa), wa);
    // A finer octave that boils in place (a circle in noise space loops).
    float D = noise3(fp * 1.9 + circ * 0.45 + vec3(4.7, 1.9, 8.3)) - 0.5;
    F = (F - 0.5) * 1.5 + 0.5 + D * 0.32;

    // 3. Flame tongues streaming out from the pupil axis: noise in
    //    cylindrical coordinates (direction round the axis, distance from
    //    it), stretched along the radius and scrolled outward.
    float rho = length(e.xy);
    vec2 rad = f.xy / max(length(f.xy), 1e-3);
    vec2 ang = rad * (sw * 1.25) + warp.xy * 1.3;
    float rz = rho * sw * 0.7 + e.z * 0.9 + warp.z * 0.8;
    float S = mix(noise3(vec3(ang + 9.1, rz - pb * 1.5)), noise3(vec3(ang, rz - pa * 1.5)), wa);
    S = (S - 0.5) * 1.5 + 0.5 + D * 0.38;
    float streak = smoothstep(0.2, 0.8, rho);         // no tongues on the axis itself
    float fl = smoothstep(0.42, 0.88, mix(F, S, 0.55 * streak) + 0.08 * F);
    float ridge = 1.0 - abs(2.0 * S - 1.0);
    float tongue = ridge * ridge * streak * smoothstep(0.35, 0.7, F);

    // 4. The core: a hot heart at the centre, and heat round the pupil axis.
    float sc = s * recoil;
    float core = exp(-sc * sc / (coreR * coreR));
    float axisHeat = exp(-rho * rho * 2.4 * recoil) * (0.45 + 0.55 * smoothstep(-0.6, 0.7, e.z));

    // 5. Smoke: dark curling threads where the boil and the tongues both
    //    cross their middle (two isosurfaces meet in a line, not a sheet),
    //    so the smoke rides the same flow as the flames.
    vec2 vd = vec2(F, S) - 0.5;
    float vein = exp(-dot(vd, vd) * 55.0) * smoothstep(0.3, 0.65, s) * streak;

    // 6. The skin: a thin shell of low flame. Rays graze it longest at the
    //    limb, so the edge of the ball glows instead of going hollow.
    float shell = smoothstep(0.76, 0.97, s) * (0.35 + 0.65 * smoothstep(0.3, 0.75, F));

    float dens = 0.06 + 1.1 * fl + 0.5 * tongue + 0.7 * core + 0.55 * shell;
    dens *= cut * (1.0 - 0.35 * wince * (1.0 - depth));
    float t = 0.06 + 0.5 * axisHeat + 0.35 * core + 0.4 * fl + 0.22 * tongue
            + 0.16 * heat - 0.25 * vein;
    t = 1.3 * (1.0 - exp(-1.15 * max(t, 0.0)));        // soft cap: hot, rarely white
    t *= 0.62 + 0.38 * wake;
    float emit = dens * (1.0 + 0.12 * breath * core);
    // Each step integrated exactly for constant emission and absorption
    // across it: a thick step no longer overshoots, so coarse steps and the
    // dither stay calm.
    float sigma = 1.6 * dens + 2.2 * vein + 0.05;
    float a = 1.0 - exp(-sigma * dt);
    col += trans * fire(t) * (emit / sigma) * a;
    trans *= 1.0 - a;
  }
  // Exposure: each colour sample is weighted by emit / sigma (about 0.6),
  // so col tops out near 0.6 of the palette where the ball is thick.
  vec3 ecol = col * (3.45 + 0.8 * heat) * (0.55 + 0.45 * wake);
  ecol *= 1.0 + 0.35 * (1.0 - nz) * (1.0 - nz);         // the short paths at the limb still burn
  ecol = ecol / (1.0 + 0.25 * ecol);                   // soft shoulder, keeps the hue

  // ---- The surface. ----
  // Painted by position, not normal, so a morph bends the slit only gently.
  vec3 l = vec3(dot(sp, rt), dot(sp, up), dot(sp, A)); // surface point, eye frame
  float zl = max(sp.z, 0.08);

  // The solid pupil, painted on the surface so it foreshortens with the ball.
  float pxl = pq / zl;
  float sh = slitHalf(l.y, pupilW);
  float pd = abs(l.x) - sh;
  float onFront = smoothstep(0.0, 0.12, l.z);
  float vmask = (1.0 - smoothstep(SLIT_H - 0.08, SLIT_H, abs(l.y))) * onFront;
  float pupil = (1.0 - smoothstep(-pxl, pxl, pd)) * vmask;
  // Ink: a soft dark falloff out of the slit. Lip: a thin hot line just
  // beyond it, where the fire meets the cut.
  float ink = (1.0 - smoothstep(0.0, 0.05 + 0.03 * pupilW, pd)) * vmask * (1.0 - pupil);
  float lw = 0.018 + pxl * 0.7;
  float ld = (pd - 0.034) / lw;
  float lip = exp(-ld * ld) * vmask * (1.0 - pupil);
  ecol *= 1.0 - 0.75 * ink;
  ecol += fire(0.95 + 0.12 * heat) * lip * (0.3 + 0.35 * heat + 0.15 * abs(pv)) * wake;
  vec3 pupilCol = mix(vec3(0.0), uC0 * 0.06, smoothstep(-sh * 0.9, 0.0, pd));
  ecol = mix(ecol, pupilCol, pupil);

  // Limb darkening, a fresnel rim of the mood's glow, and a faint sheen
  // that slides over the cornea as the ball turns.
  ecol *= mix(0.72, 1.0, sqrt(nz));
  float fres = pow(1.0 - nz, 3.5);
  ecol += uCG * fres * (0.3 + 0.45 * heat) * (0.4 + 0.6 * wake);
  vec3 Lgt = normalize(vec3(-0.45, 0.6, 0.65) + vec3(uGaze * 0.15, 0.0));
  vec3 hv = normalize(Lgt + vec3(0.0, 0.0, 1.0));
  float spec = pow(max(dot(n, hv), 0.0), 60.0);
  spec *= 0.7 + 0.3 * noise(q * 3.0 + circ.xy * 0.6);
  ecol += mix(uCG, vec3(1.0), 0.5) * spec * 0.16 * (1.0 - pupil * 0.5);

  return vec4(ecol * inside, inside);
}
