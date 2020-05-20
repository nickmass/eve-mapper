#version 420

in vec3 v_color;
in vec2 v_normal;

out vec4 color;

void main() {
    float alpha = (1.0 - smoothstep(0.4, 1.0, length(v_normal))) * 0.8;
    color = vec4(v_color, alpha);
}
