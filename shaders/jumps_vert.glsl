#version 420

in vec3 position;
in vec2 normal;
in vec3 color;

out vec3 v_color;
out vec2 v_normal;

uniform mat3 map_view_matrix;
uniform mat3 map_scale_matrix;

void main() {
   vec3 view_position = map_view_matrix * vec3(position.xy, 1.0);
   vec3 width_position = view_position + vec3((normal * 0.005) / 2.0, 0.0);
   vec3 scaled_position = map_scale_matrix * width_position;
   v_color = color;
   v_normal = normal;
   gl_Position = vec4(scaled_position.xy, scaled_position.z * position.z, scaled_position.z);
}
