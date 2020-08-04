#version 420

in vec2 position;
in vec2 uv;

out vec2 v_uv;
out float v_alpha;

uniform vec2 window_size;

void main () {
  v_uv = uv;
  gl_Position = vec4((vec2(position.x, window_size.y - position.y) / window_size) * 2.0 - 1.0, 1.0, 1.0);
}
