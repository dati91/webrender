#version 120

varying vec2 v_TexCoord;

uniform sampler2D sLayers;
uniform sampler2D sRenderTasks;
uniform sampler2D sPrimGeometry;
uniform sampler2D sData16;

void main() {
    gl_FragColor = texture2D(sLayers, v_TexCoord);
}
