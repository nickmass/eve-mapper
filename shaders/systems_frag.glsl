#version 420

in vec3 v_color;
in vec2 v_center;
in vec2 v_position;

out vec4 color;

void main() {
    float dist = distance(v_position, vec2(0.0, 0.0));
    float in_dist = pow((1.0 - (1.0 / dist)), 2);
    color = vec4(v_color, in_dist);
}
