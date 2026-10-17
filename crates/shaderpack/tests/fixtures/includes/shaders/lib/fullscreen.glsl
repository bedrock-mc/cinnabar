// Vertex body shared by every full-screen pass.
out vec2 uv;

void main() {
    uv = gl_MultiTexCoord0.xy;
    gl_Position = ftransform();
}
