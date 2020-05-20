#version 420

in vec2 position;
in vec2 uv;
in vec3 color;
in float alpha;

out vec2 v_uv;
out vec3 v_color;
out float v_alpha;

uniform vec2 window_size;

void main () {
  v_uv = uv;
  v_color = color;
  v_alpha = alpha;
  gl_Position = vec4((position.xy / window_size) * 2.0 - 1.0, 1.0, 1.0);
}
