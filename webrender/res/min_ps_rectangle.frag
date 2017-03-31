#version 150
//======================================================================================
// Fragment shader attributes and uniforms
//======================================================================================
    precision highp float;

    #define varying in

    // Uniform inputs

    // Fragment shader outputs
    out vec4 oFragColor;

// Base shader: ps_rectangle

//======================================================================================
// PS Rectangle FS
//======================================================================================

varying vec4 vColor;

void main(void) {
    float alpha = 1.0;

    oFragColor = vColor * vec4(1.0, 1.0, 1.0, alpha);
}
