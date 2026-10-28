// main(): the aura under the body, then the app's fades.
// This file is pasted after common.glsl, body.glsl and aura.glsl.

void main() {
  float px = 1.0 / min(uRes.x, uRes.y);
  vec2 p = (fragXY - 0.5 * uRes) * px;
  p -= uCenter;
  // Grow from small while opening, shrink away while closing.
  p /= uSize;
  px /= uSize;
  float heat = clamp(uHeat + uFlare * 0.8, 0.0, 1.8);
  float wake = clamp(uWide, 0.0, 1.2);

  vec4 o = aura(p, px, heat, wake);

  // Fade the light out before it reaches the window's edges.
  vec2 uv = fragXY / uRes;
  float border = smoothstep(0.0, 0.18, min(uv.x, 1.0 - uv.x)) * smoothstep(0.0, 0.18, min(uv.y, 1.0 - uv.y));
  o *= border * border;

  // The body over the aura, premultiplied "over".
  vec4 b = body(p, px, heat, wake);
  vec4 c = b + o * (1.0 - b.a);

  float dimK = mix(1.0, 0.28, uDim);
  c.rgb *= dimK;
  c.a *= mix(1.0, 0.75, uDim);
  // Premultiplied, so fading is one multiply of colour and alpha together.
  outColor = c * uFade;
}
