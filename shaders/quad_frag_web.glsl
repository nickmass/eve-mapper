precision highp float;

varying vec2 v_uv;

uniform sampler2D u_texture_atlas;
uniform bool u_textured;
uniform vec4 u_color;

void main () {
  vec4 color = vec4(pow(u_color.xyz, vec3(1.0 / 2.2)), u_color.w);
  if (u_textured) {
    gl_FragColor = texture2D(u_texture_atlas, v_uv) * color;
  } else {
    gl_FragColor = color;
  }
}

