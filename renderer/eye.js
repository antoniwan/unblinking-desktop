// The eye itself: one full-window quad and one fragment shader.
// Everything visible (iris fire, slit pupil, wreath of flame, glow)
// is computed per pixel from a handful of uniforms the mind drives.

const VERT = `
attribute vec2 aPos;
void main() { gl_Position = vec4(aPos, 0.0, 1.0); }
`;

const FRAG = `
precision highp float;

uniform vec2 uRes;
uniform float uFlow;    // 0..1 looping phase that drives flame drift
uniform vec2 uGaze;     // -1..1, +y is up
uniform float uPupil;   // 0 = razor slit, 1 = wide
uniform float uHeat;    // 0..1 flame strength
uniform float uFlare;   // 0..1 sudden burst, decays
uniform float uWide;    // how far the eye is open
uniform float uDim;     // 0 awake, 1 blinded
uniform vec2 uCenter;   // eye center offset, in eye units

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
  for (int i = 0; i < 5; i++) {
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

void main() {
  vec2 p = (gl_FragCoord.xy - 0.5 * uRes) / min(uRes.x, uRes.y);
  p -= uCenter;
  float heat = clamp(uHeat + uFlare * 0.8, 0.0, 1.6);

  // The almond: half width a, half height b, pointed at the corners.
  float a = 0.29;
  float b = 0.115 * uWide;
  float xn = p.x / a;
  float edge = b * pow(max(0.0, 1.0 - xn * xn), 0.8);
  float de = max(abs(p.y) - edge, abs(p.x) - a);
  float inside = 1.0 - smoothstep(-0.004, 0.004, de);

  // ---- Outside: dark rim, wreath of flame, glow ----
  vec2 ab = vec2(a, max(b, 0.02));
  float f = length(p / ab);
  vec2 grad = p / (ab * ab) / max(f, 1e-4);
  float d = (f - 1.0) / max(length(grad), 1e-4);

  vec2 outward = normalize(p + vec2(0.0, 1e-4));
  float n = flowNoise(p * vec2(7.0, 5.0), -(outward * 0.9 + vec2(0.0, 0.7)));
  float tongues = flowNoise(p * vec2(16.0, 4.5) + 7.1, -vec2(0.0, 1.4));
  float fn = n * 0.75 + tongues * 0.45;

  float reach = mix(0.06, 0.17, clamp(heat, 0.0, 1.0)) + uFlare * 0.05;
  reach *= 0.7 + 0.9 * smoothstep(-0.2, 0.35, p.y);   // taller above than below
  float fv = fn * 1.5 - d / reach;
  float flame = clamp(fv, 0.0, 1.5);
  vec3 fcol = fire(flame * 1.15 + heat * 0.1);
  float falpha = smoothstep(0.02, 0.4, flame);

  float rim = (1.0 - smoothstep(0.004, 0.026, de)) * step(0.0, de);
  fcol = mix(fcol, vec3(0.05, 0.005, 0.0), rim);
  falpha = max(falpha, rim);

  float glow = exp(-max(d, 0.0) * 10.0) * (0.18 + 0.32 * heat);
  vec3 glowCol = vec3(1.0, 0.32, 0.05);
  vec3 outPm = fcol * falpha + glowCol * glow * (1.0 - falpha);
  float outA = falpha + glow * (1.0 - falpha);

  // ---- Inside: churning iris fire and the slit ----
  vec2 c = vec2(uGaze.x * a * 0.55, uGaze.y * b * 0.45);
  vec2 pc = p - c;
  vec2 radial = normalize(pc + vec2(1e-4, 0.0));
  float dpup = length(pc * vec2(1.0, 0.8));
  float swirl = flowNoise(pc * 9.0, -radial * 0.8);
  float streak = noise(radial * 9.0 + 3.3);
  float it = 1.08 - dpup * 3.7 + (swirl - 0.5) * 0.9 + (streak - 0.5) * 0.35 + heat * 0.15;
  vec3 ecol = fire(it);

  float h = b * 1.25;
  float w = mix(0.006, 0.05, clamp(uPupil, 0.0, 1.0));
  float py = clamp(pc.y / h, -1.0, 1.0);
  float slit = w * sqrt(max(0.0, 1.0 - py * py));
  float pd = abs(pc.x) - slit;
  float pupil = (1.0 - smoothstep(-0.002, 0.003, pd)) * (1.0 - smoothstep(h * 0.9, h, abs(pc.y)));
  float ring = exp(-max(pd, 0.0) * 110.0) * (1.0 - pupil);
  ecol += vec3(1.0, 0.85, 0.5) * ring * (0.45 + 0.4 * heat);
  ecol = mix(ecol, vec3(0.015, 0.0, 0.0), pupil);
  ecol *= mix(0.12, 1.0, smoothstep(0.0, 0.032, -de));   // dark at the edge

  // Fade the fire out before it reaches the window's edges.
  vec2 uv = gl_FragCoord.xy / uRes;
  float border = smoothstep(0.0, 0.14, min(uv.x, 1.0 - uv.x)) * smoothstep(0.0, 0.16, 1.0 - uv.y);
  outPm *= border;
  outA *= border;

  vec3 pm = mix(outPm, ecol, inside);
  float alpha = mix(outA, 1.0, inside);

  float dimK = mix(1.0, 0.28, uDim);
  pm *= dimK;
  alpha *= mix(1.0, 0.75, uDim);
  gl_FragColor = vec4(pm, alpha);
}
`;

function compile(gl, type, src) {
  const s = gl.createShader(type);
  gl.shaderSource(s, src);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
    throw new Error('shader: ' + gl.getShaderInfoLog(s));
  }
  return s;
}

export function createEye(canvas) {
  const gl = canvas.getContext('webgl', { alpha: true, premultipliedAlpha: true, antialias: false });
  if (!gl) throw new Error('WebGL is not available');

  const prog = gl.createProgram();
  gl.attachShader(prog, compile(gl, gl.VERTEX_SHADER, VERT));
  gl.attachShader(prog, compile(gl, gl.FRAGMENT_SHADER, FRAG));
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
    throw new Error('program: ' + gl.getProgramInfoLog(prog));
  }
  gl.useProgram(prog);

  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  const aPos = gl.getAttribLocation(prog, 'aPos');
  gl.enableVertexAttribArray(aPos);
  gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);

  const u = {};
  for (const name of ['uRes', 'uFlow', 'uGaze', 'uPupil', 'uHeat', 'uFlare', 'uWide', 'uDim', 'uCenter']) {
    u[name] = gl.getUniformLocation(prog, name);
  }

  function resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const w = Math.round(canvas.clientWidth * dpr);
    const h = Math.round(canvas.clientHeight * dpr);
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    gl.viewport(0, 0, canvas.width, canvas.height);
  }

  return {
    render(s) {
      resize();
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.uniform2f(u.uRes, canvas.width, canvas.height);
      gl.uniform1f(u.uFlow, s.flow);
      gl.uniform2f(u.uGaze, s.gaze.x, s.gaze.y);
      gl.uniform1f(u.uPupil, s.pupil);
      gl.uniform1f(u.uHeat, s.heat);
      gl.uniform1f(u.uFlare, s.flare);
      gl.uniform1f(u.uWide, s.wide);
      gl.uniform1f(u.uDim, s.dim);
      gl.uniform2f(u.uCenter, s.center.x, s.center.y);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    },
  };
}
