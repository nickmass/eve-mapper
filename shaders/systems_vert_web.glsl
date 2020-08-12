precision highp float;

attribute vec2 a_position;
attribute vec4 a_color;
attribute vec4 a_highlight;
attribute vec2 a_center;
attribute float a_scale;
attribute float a_radius;

varying vec4 v_color;
varying vec4 v_highlight;
varying vec2 v_center;
varying vec2 v_position;

uniform mat3 u_map_view_matrix;
uniform mat3 u_map_scale_matrix;
uniform float u_zoom;

void main() {
   float clamp_zoom = min(max(u_zoom / a_radius, 1.0), a_radius) * a_scale;
   vec3 view_position = (u_map_view_matrix * vec3(a_center, 1.0)) +  vec3(a_position * 0.004 * clamp_zoom, 0.0);
   vec3 scaled_position = u_map_scale_matrix * view_position;
   v_color = a_color;
   v_highlight = a_highlight;
   v_position = a_position;
   v_center = a_center;
   gl_Position = vec4(scaled_position, scaled_position.z);
}
