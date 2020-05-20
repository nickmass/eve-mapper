#version 420

in vec2 position;
in vec2 normal;
in vec3 color;
in vec2 center;

out vec3 v_color;
out vec2 v_center;
out vec2 v_position;

uniform mat3 map_view_matrix;
uniform mat3 map_scale_matrix;
uniform float zoom;

void main() {
   float clamp_zoom = max(zoom / 3.0, 1.0);
   vec3 view_position = (map_view_matrix * vec3(center, 1.0)) +  vec3(position * 0.004 * clamp_zoom, 0.0);
   vec3 scaled_position = map_scale_matrix * view_position;
   v_color = color;
   v_position = position;
   v_center = center;
   gl_Position = vec4(scaled_position, scaled_position.z);
}
