precision highp float;

varying vec2 v_uv;

uniform sampler2D u_texture_atlas;
uniform bool u_textured;
uniform vec4 u_color;

void main () {
  if (u_textured) {
    gl_FragColor = texture2D(u_texture_atlas, v_uv) * u_color;
  } else {
    gl_FragColor = u_color;
  }
}

