precision highp float;

varying vec3 v_color;
varying vec2 v_normal;

void main() {
    float alpha = (1.0 - smoothstep(0.4, 1.0, length(v_normal))) * 0.8;
    gl_FragColor = vec4(v_color, alpha);
}
