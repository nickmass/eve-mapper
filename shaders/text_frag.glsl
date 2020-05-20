#version 420

in vec2 v_uv;
in vec3 v_color;
in float v_alpha;

out vec4 color;

uniform sampler2D font_atlas;

void main () {
  float coverage = texture(font_atlas, v_uv).x;
  color = vec4(v_color * coverage, coverage * v_alpha);
}
