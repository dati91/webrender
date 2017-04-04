#version 150
#define WR_MAX_VERTEX_TEXTURE_WIDTH 1024
/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//======================================================================================
// Vertex shader attributes and uniforms
//======================================================================================
    #define varying out

    // Uniform inputs
    uniform mat4 uTransform;       // Orthographic projection
    uniform float uDevicePixelRatio;

    // Attribute inputs
    in vec3 aPosition;

//======================================================================================
// Shared shader uniforms
//======================================================================================
// Base shader: ps_rectangle

#define VECS_PER_LAYER             13
#define VECS_PER_RENDER_TASK        3
#define VECS_PER_PRIM_GEOM          2

uniform sampler2D sLayers;
uniform sampler2D sRenderTasks;
uniform sampler2D sPrimGeometry;

uniform sampler2D sData16;

// Instanced attributes
in int aGlobalPrimId;
in int aPrimitiveAddress;
in int aTaskIndex;
in int aClipTaskIndex;
in int aLayerIndex;
in int aElementIndex;
in ivec2 aUserData;
in int aZIndex;

// get_fetch_uv is a macro to work around a macOS Intel driver parsing bug.
// TODO: convert back to a function once the driver issues are resolved, if ever.
// https://github.com/servo/webrender/pull/623
// https://github.com/servo/servo/issues/13953
#define get_fetch_uv(i, vpi)  ivec2(vpi * (i % (WR_MAX_VERTEX_TEXTURE_WIDTH/vpi)), i / (WR_MAX_VERTEX_TEXTURE_WIDTH/vpi))

struct RectWithSize {
    vec2 p0;
    vec2 size;
};

struct RectWithEndpoint {
    vec2 p0;
    vec2 p1;
};

RectWithEndpoint to_rect_with_endpoint(RectWithSize rect) {
    RectWithEndpoint result;
    result.p0 = rect.p0;
    result.p1 = rect.p0 + rect.size;

    return result;
}


vec2 clamp_rect(vec2 point, RectWithSize rect) {
    return clamp(point, rect.p0, rect.p0 + rect.size);
}

vec2 clamp_rect(vec2 point, RectWithEndpoint rect) {
    return clamp(point, rect.p0, rect.p1);
}

// Clamp 2 points at once.
vec4 clamp_rect(vec4 points, RectWithSize rect) {
    return clamp(points, rect.p0.xyxy, rect.p0.xyxy + rect.size.xyxy);
}

vec4 clamp_rect(vec4 points, RectWithEndpoint rect) {
    return clamp(points, rect.p0.xyxy, rect.p1.xyxy);
}

struct Layer {
    mat4 transform;
    mat4 inv_transform;
    RectWithSize local_clip_rect;
    vec4 screen_vertices[4];
};

Layer fetch_layer(int index) {
    Layer layer;

    // Create a UV base coord for each 8 texels.
    // This is required because trying to use an offset
    // of more than 8 texels doesn't work on some versions
    // of OSX.
    ivec2 uv = get_fetch_uv(index, VECS_PER_LAYER);
    ivec2 uv0 = ivec2(uv.x + 0, uv.y);
    ivec2 uv1 = ivec2(uv.x + 8, uv.y);

    layer.transform[0] = texelFetchOffset(sLayers, uv0, 0, ivec2(0, 0));
    layer.transform[1] = texelFetchOffset(sLayers, uv0, 0, ivec2(1, 0));
    layer.transform[2] = texelFetchOffset(sLayers, uv0, 0, ivec2(2, 0));
    layer.transform[3] = texelFetchOffset(sLayers, uv0, 0, ivec2(3, 0));

    layer.inv_transform[0] = texelFetchOffset(sLayers, uv0, 0, ivec2(4, 0));
    layer.inv_transform[1] = texelFetchOffset(sLayers, uv0, 0, ivec2(5, 0));
    layer.inv_transform[2] = texelFetchOffset(sLayers, uv0, 0, ivec2(6, 0));
    layer.inv_transform[3] = texelFetchOffset(sLayers, uv0, 0, ivec2(7, 0));

    vec4 clip_rect = texelFetchOffset(sLayers, uv1, 0, ivec2(0, 0));
    layer.local_clip_rect = RectWithSize(clip_rect.xy, clip_rect.zw);

    layer.screen_vertices[0] = texelFetchOffset(sLayers, uv1, 0, ivec2(1, 0));
    layer.screen_vertices[1] = texelFetchOffset(sLayers, uv1, 0, ivec2(2, 0));
    layer.screen_vertices[2] = texelFetchOffset(sLayers, uv1, 0, ivec2(3, 0));
    layer.screen_vertices[3] = texelFetchOffset(sLayers, uv1, 0, ivec2(4, 0));

    return layer;
}

struct RenderTaskData {
    vec4 data0;
    vec4 data1;
    vec4 data2;
};

RenderTaskData fetch_render_task(int index) {
    RenderTaskData task;

    ivec2 uv = get_fetch_uv(index, VECS_PER_RENDER_TASK);

    task.data0 = texelFetchOffset(sRenderTasks, uv, 0, ivec2(0, 0));
    task.data1 = texelFetchOffset(sRenderTasks, uv, 0, ivec2(1, 0));
    task.data2 = texelFetchOffset(sRenderTasks, uv, 0, ivec2(2, 0));

    return task;
}

struct AlphaBatchTask {
    vec2 screen_space_origin;
    vec2 render_target_origin;
    vec2 size;
    float render_target_layer_index;
};

AlphaBatchTask fetch_alpha_batch_task(int index) {
    RenderTaskData data = fetch_render_task(index);

    AlphaBatchTask task;
    task.render_target_origin = data.data0.xy;
    task.size = data.data0.zw;
    task.screen_space_origin = data.data1.xy;
    task.render_target_layer_index = data.data1.z;

    return task;
}

struct ClipArea {
    vec4 task_bounds;
    vec4 screen_origin_target_index;
    vec4 inner_rect;
};

ClipArea fetch_clip_area(int index) {
    ClipArea area;

    if (index == 0x7FFFFFFF) { //special sentinel task index
        area.task_bounds = vec4(0.0, 0.0, 0.0, 0.0);
        area.screen_origin_target_index = vec4(0.0, 0.0, 0.0, 0.0);
        area.inner_rect = vec4(0.0);
    } else {
        RenderTaskData task = fetch_render_task(index);
        area.task_bounds = task.data0;
        area.screen_origin_target_index = task.data1;
        area.inner_rect = task.data2;
    }

    return area;
}

struct PrimitiveGeometry {
    RectWithSize local_rect;
    RectWithSize local_clip_rect;
};

PrimitiveGeometry fetch_prim_geometry(int index) {
    PrimitiveGeometry pg;

    ivec2 uv = get_fetch_uv(1-index, VECS_PER_PRIM_GEOM);

    vec4 local_rect = texelFetchOffset(sPrimGeometry, uv, 0, ivec2(0, 0));
    pg.local_rect = RectWithSize(local_rect.xy, local_rect.zw);
    vec4 local_clip_rect = texelFetchOffset(sPrimGeometry, uv, 0, ivec2(1, 0));
    pg.local_clip_rect = RectWithSize(local_clip_rect.xy, local_clip_rect.zw);

    return pg;
}

struct PrimitiveInstance {
    int global_prim_index;
    int specific_prim_index;
    int render_task_index;
    int clip_task_index;
    int layer_index;
    int sub_index;
    int z;
    ivec2 user_data;
};

PrimitiveInstance fetch_prim_instance() {
    PrimitiveInstance pi;

    pi.global_prim_index = aGlobalPrimId;
    pi.specific_prim_index = aPrimitiveAddress;
    pi.render_task_index = aTaskIndex;
    pi.clip_task_index = aClipTaskIndex;
    pi.layer_index = aLayerIndex;
    pi.sub_index = aElementIndex;
    pi.user_data = aUserData;
    pi.z = aZIndex;

    return pi;
}

struct Primitive {
    Layer layer;
    ClipArea clip_area;
    AlphaBatchTask task;
    RectWithSize local_rect;
    RectWithSize local_clip_rect;
    int prim_index;
    // when sending multiple primitives of the same type (e.g. border segments)
    // this index allows the vertex shader to recognize the difference
    int sub_index;
    ivec2 user_data;
    float z;
};

Primitive load_primitive_custom(PrimitiveInstance pi) {
    Primitive prim;

    prim.layer = fetch_layer(pi.layer_index);
    prim.clip_area = fetch_clip_area(pi.clip_task_index);
    prim.task = fetch_alpha_batch_task(pi.render_task_index);

    PrimitiveGeometry pg = fetch_prim_geometry(pi.global_prim_index);
    prim.local_rect = pg.local_rect;
    prim.local_clip_rect = pg.local_clip_rect;

    prim.prim_index = pi.specific_prim_index;
    prim.sub_index = pi.sub_index;
    prim.user_data = pi.user_data;
    prim.z = float(pi.z);

    return prim;
}

Primitive load_primitive() {
    PrimitiveInstance pi = fetch_prim_instance();

    return load_primitive_custom(pi);
}

struct VertexInfo {
    RectWithEndpoint local_rect;
    vec2 local_pos;
    vec2 screen_pos;
};

VertexInfo write_vertex(RectWithSize instance_rect,
                        RectWithSize local_clip_rect,
                        float z,
                        Layer layer,
                        AlphaBatchTask task) {
    RectWithEndpoint local_rect = to_rect_with_endpoint(instance_rect);

    // Select the corner of the local rect that we are processing.
    vec2 local_pos = mix(local_rect.p0, local_rect.p1, aPosition.xy);

    // xy = top left corner of the local rect, zw = position of current vertex.
    vec4 local_p0_pos = vec4(local_rect.p0, local_pos);

    // Clamp to the two local clip rects.
    local_p0_pos = clamp_rect(local_p0_pos, local_clip_rect);
    local_p0_pos = clamp_rect(local_p0_pos, layer.local_clip_rect);

    // Transform the top corner and current vertex to world space.
    vec4 world_p0 = layer.transform * vec4(local_p0_pos.xy, 0.0, 1.0);
    world_p0.xyz /= world_p0.w;
    vec4 world_pos = layer.transform * vec4(local_p0_pos.zw, 0.0, 1.0);
    world_pos.xyz /= world_pos.w;

    // Convert the world positions to device pixel space. xy=top left corner. zw=current vertex.
    vec4 device_p0_pos = vec4(world_p0.xy, world_pos.xy) * uDevicePixelRatio;

    // Calculate the distance to snap the vertex by (snap top left corner).
    vec2 snap_delta = device_p0_pos.xy - floor(device_p0_pos.xy + 0.5);

    // Apply offsets for the render task to get correct screen location.
    vec2 final_pos = device_p0_pos.zw -
                     snap_delta -
                     task.screen_space_origin +
                     task.render_target_origin;

    //gl_Position = uTransform * vec4(final_pos, z, 1.0);
    //gl_Position = vec4(final_pos, z, 1.0);
    //gl_Position = uTransform * vec4(aPosition.xy, z, 1.0);
    //gl_Position = vec4(aPosition.xy, z, 1.0);
    //vec2 pos = vec2(local_pos.x / local_clip_rect.size.x, local_pos.y / local_clip_rect.size.y);
    vec2 pos = vec2(local_pos.x / 1024.0, local_pos.y / 768.0);
    //vec2 pos = vec2(local_pos.x, local_pos.y);
    gl_Position = vec4(pos, z, 1.0);
    VertexInfo vi = VertexInfo(local_rect, local_p0_pos.zw, device_p0_pos.zw);
    return vi;
}

struct Rectangle {
    vec4 color;
};

Rectangle fetch_rectangle(int index) {
    Rectangle rect;

    ivec2 uv = get_fetch_uv(index, 1);

    rect.color = texelFetchOffset(sData16, uv, 0, ivec2(0, 0));

    return rect;
}

//======================================================================================
// PS Rectangle VS
//======================================================================================

varying vec4 vColor;

void main(void) {
    Primitive prim = load_primitive();
    Rectangle rect = fetch_rectangle(prim.prim_index);
    vColor = rect.color;
    //vColor = vec4(aPosition.xy, prim.prim_index, 1.0);
    //RectWithEndpoint local_rect = to_rect_with_endpoint(prim.local_rect);
    //vColor = vec4(mix(local_rect.p0, local_rect.p1, aPosition.xy),0.0, 1.0);
    //vColor = vec4(prim.local_rect.p0.x, prim.local_rect.p0.y, prim.prim_index, 1.0);
    VertexInfo vi = write_vertex(prim.local_rect,
                                 prim.local_clip_rect,
                                 prim.z,
                                 prim.layer,
                                 prim.task);
}
