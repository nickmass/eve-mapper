precision highp float;

varying vec4 v_color;
varying vec4 v_highlight;
varying vec2 v_center;
varying vec2 v_position;

void main() {
    float dist = distance(v_position, vec2(0.0, 0.0));
    float in_dist = clamp(1.0 - pow(dist + 0.4, 20.0), 0.0, 1.0);
    float hi_dist = clamp(1.0 - pow(dist + 0.3, 2.0), 0.0, 1.0);
    vec4 v_color_gamma = pow(v_color, vec4(1.0 / 2.0));
    vec4 v_highlight_gamma = pow(v_highlight, vec4(1.0 / 2.0));
    vec4 color = (vec4(v_color_gamma.xyz, in_dist) * in_dist) + (vec4(v_highlight_gamma.xyz, hi_dist * v_highlight_gamma.w) * (1.0 - in_dist));
    gl_FragColor = color * v_color_gamma.w;
}
