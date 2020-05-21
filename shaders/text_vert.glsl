#version 420

in vec2 position;
in vec2 uv;
in vec4 color;

out vec2 v_uv;
out vec4 v_color;
out float v_alpha;

uniform vec2 window_size;

void main () {
  v_uv = uv;
  v_color = color;
  gl_Position = vec4((position.xy / window_size) * 2.0 - 1.0, 1.0, 1.0);
}
