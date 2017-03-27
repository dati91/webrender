#version 120

attribute vec2 a_Pos;
attribute vec2 a_TexCoord;
varying vec2 v_TexCoord;

void main() {
    //v_TexCoord = a_TexCoord;
    v_TexCoord = vec2(a_TexCoord.x, 1.0f - a_TexCoord.y);
    gl_Position = vec4(a_Pos, 0.0, 1.0);
}
