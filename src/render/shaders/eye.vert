#version 450
// One triangle big enough to cover the whole window; no vertex buffer needed.
// Vertex 0, 1, 2 land at (-1,-1), (3,-1), (-1,3) in clip space.
void main() {
  vec2 pos = vec2(float((gl_VertexIndex << 1) & 2), float(gl_VertexIndex & 2)) * 2.0 - 1.0;
  gl_Position = vec4(pos, 0.0, 1.0);
}
