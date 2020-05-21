#version 420

in vec2 v_uv;
in vec4 v_color;

out vec4 color;

uniform sampler2D font_atlas;

void main () {
  float coverage = texture(font_atlas, v_uv).x;
  color = v_color * coverage;
}
