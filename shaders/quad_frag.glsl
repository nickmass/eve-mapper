#version 420

in vec2 v_uv;

out vec4 f_color;

uniform sampler2D texture_atlas;
uniform bool textured;
uniform vec4 color;

void main () {
  if (textured) {
    f_color = texture(texture_atlas, v_uv) * color;
  } else {
    f_color = color;
  }
}

