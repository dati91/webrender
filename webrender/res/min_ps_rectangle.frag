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
    vec4 vColor_pow = vec4(pow(vColor.x, 2.2), pow(vColor.y, 2.2), pow(vColor.z, 2.2), alpha);
    oFragColor = vColor_pow * vec4(1.0, 1.0, 1.0, alpha);
}
