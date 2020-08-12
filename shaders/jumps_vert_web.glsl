precision highp float;

attribute vec3 a_position;
attribute vec2 a_normal;
attribute vec3 a_color;

varying vec3 v_color;
varying vec2 v_normal;

uniform mat3 u_map_view_matrix;
uniform mat3 u_map_scale_matrix;

void main() {
   vec3 view_position = u_map_view_matrix * vec3(a_position.xy, 1.0);
   vec3 width_position = view_position + vec3((a_normal * 0.005) / 2.0, 0.0);
   vec3 scaled_position = u_map_scale_matrix * width_position;
   v_color = a_color;
   v_normal = a_normal;
   gl_Position = vec4(scaled_position.xy, scaled_position.z * a_position.z, scaled_position.z);
}
