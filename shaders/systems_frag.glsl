#version 420

in vec4 v_color;
in vec4 v_highlight;
in vec2 v_center;
in vec2 v_position;

out vec4 color;

void main() {
    float dist = distance(v_position, vec2(0.0, 0.0));
    float in_dist = pow((1.0 - (1.0 / dist)), 4.0);
    float hi_dist = pow((1.0 - (1.0 / dist)), 2.0);
    color = (vec4(v_color.xyz, in_dist) * clamp(in_dist, 0.0, 1.0)) + (vec4(v_highlight.xyz, hi_dist * v_highlight.w) * clamp(1.0 - in_dist, 0.0, 1.0));
    color = color * v_color.w;
}
