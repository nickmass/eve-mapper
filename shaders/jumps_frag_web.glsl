precision highp float;

varying vec3 v_color;
varying vec2 v_normal;

void main() {
    vec3 color = pow(v_color, vec3(1.0 / 2.2));
    float alpha = (1.0 - smoothstep(0.4, 1.0, length(v_normal))) * 0.8;
    gl_FragColor = vec4(color, alpha);
}
