precision highp float;

varying vec2 v_uv;
varying vec4 v_color;

uniform sampler2D u_font_atlas;

void main () {
  float coverage = texture2D(u_font_atlas, v_uv).w;
  gl_FragColor = vec4(v_color.xyz, coverage * v_color.w);
}
