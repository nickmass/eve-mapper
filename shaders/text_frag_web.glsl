precision highp float;

varying vec2 v_uv;
varying vec4 v_color;

uniform sampler2D u_font_atlas;

void main () {
  float coverage = texture2D(u_font_atlas, v_uv).w;
  vec4 gamma_color = vec4(pow(v_color.xyz, vec3(1.0 / 2.2)), v_color.w);
  gl_FragColor = vec4(gamma_color.xyz, coverage * gamma_color.w);
}
