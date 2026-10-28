// ---- The body: the sphere and everything inside it. ----
// Returns premultiplied colour, with alpha = how much of this pixel the
// sphere covers. Nothing here may draw outside the sphere's silhouette.

float sphereDist(vec2 p) {
  return length(p) - R;
}

vec4 body(vec2 p, float px, float heat, float wake) {
  float rr = length(p);
  float r = rr / R;
  float dist = rr - R;

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
  return vec4(ecol * inside, inside);
}
