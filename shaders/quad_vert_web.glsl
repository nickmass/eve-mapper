precision highp float;

attribute vec2 a_position;
attribute vec2 a_uv;

varying vec2 v_uv;

uniform vec2 u_window_size;

void main () {
  v_uv = a_uv;
  gl_Position = vec4((vec2(a_position.x, u_window_size.y - a_position.y) / u_window_size) * 2.0 - 1.0, 1.0, 1.0);
}
