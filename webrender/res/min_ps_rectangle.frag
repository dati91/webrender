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
uniform sampler2D sData16;
uniform sampler2D sPrimGeometry;
varying vec4 vColor;

void main(void) {
    float alpha = 1.0;

    oFragColor = vColor * vec4(1.0, 1.0, 1.0, alpha);
    //oFragColor = vec4(1.0, 1.0, 1.0, alpha);
    //oFragColor = texture2D(sData16, vColor.xy);
    //oFragColor = texture2D(sPrimGeometry, vColor.xy);
    //oFragColor = texelFetchOffset(sData16, ivec2(vColor.z, 0), 0, ivec2(0, 0));
}
