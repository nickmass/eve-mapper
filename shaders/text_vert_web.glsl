precision highp float;

attribute vec2 a_position;
attribute vec2 a_uv;
attribute vec4 a_color;

varying vec2 v_uv;
varying vec4 v_color;

uniform vec2 u_window_size;

void main () {
  v_uv = a_uv;
  v_color = a_color;
  gl_Position = vec4((a_position.xy / u_window_size) * 2.0 - 1.0, 1.0, 1.0);
}
