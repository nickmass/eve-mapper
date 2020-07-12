#version 420

in vec4 v_color;
in vec4 v_highlight;
in vec2 v_center;
in vec2 v_position;

out vec4 color;

void main() {
    float dist = distance(v_position, vec2(0.0, 0.0));
    float in_dist = clamp(1.0 - pow(dist + 0.4, 20.0), 0.0, 1.0);
    float hi_dist = clamp(1.0 - pow(dist + 0.3, 2.0), 0.0, 1.0);
    color = (vec4(v_color.xyz, in_dist) * in_dist) + (vec4(v_highlight.xyz, hi_dist * v_highlight.w) * (1.0 - in_dist));
    color = color * v_color.w;
}
