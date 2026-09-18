// Generated GLES C ABI re-exports
use core::ffi::*;
use vantage_gles::*;

#[no_mangle]
pub unsafe extern "C" fn glMatrixMode(mode: GLenum) {
    vantage_gles::glMatrixMode(mode)
}

#[no_mangle]
pub unsafe extern "C" fn glLoadIdentity() {
    vantage_gles::glLoadIdentity()
}

#[no_mangle]
pub unsafe extern "C" fn glPushMatrix() {
    vantage_gles::glPushMatrix()
}

#[no_mangle]
pub unsafe extern "C" fn glPopMatrix() {
    vantage_gles::glPopMatrix()
}

#[no_mangle]
pub unsafe extern "C" fn glTranslatef(x: GLfloat, y: GLfloat, z: GLfloat) {
    vantage_gles::glTranslatef(x, y, z)
}

#[no_mangle]
pub unsafe extern "C" fn glTranslated(x: GLdouble, y: GLdouble, z: GLdouble) {
    vantage_gles::glTranslated(x, y, z)
}

#[no_mangle]
pub unsafe extern "C" fn glRotatef(angle: GLfloat, x: GLfloat, y: GLfloat, z: GLfloat) {
    vantage_gles::glRotatef(angle, x, y, z)
}

#[no_mangle]
pub unsafe extern "C" fn glScalef(x: GLfloat, y: GLfloat, z: GLfloat) {
    vantage_gles::glScalef(x, y, z)
}

#[no_mangle]
pub unsafe extern "C" fn glScaled(x: GLdouble, y: GLdouble, z: GLdouble) {
    vantage_gles::glScaled(x, y, z)
}

#[no_mangle]
pub unsafe extern "C" fn glOrtho(
    left: GLdouble,
    right: GLdouble,
    bottom: GLdouble,
    top: GLdouble,
    near_val: GLdouble,
    far_val: GLdouble,
) {
    vantage_gles::glOrtho(left, right, bottom, top, near_val, far_val)
}

#[no_mangle]
pub unsafe extern "C" fn glOrthof(
    left: GLfloat,
    right: GLfloat,
    bottom: GLfloat,
    top: GLfloat,
    near_val: GLfloat,
    far_val: GLfloat,
) {
    vantage_gles::glOrthof(left, right, bottom, top, near_val, far_val)
}

#[no_mangle]
pub unsafe extern "C" fn glFrustum(
    left: GLdouble,
    right: GLdouble,
    bottom: GLdouble,
    top: GLdouble,
    near_val: GLdouble,
    far_val: GLdouble,
) {
    vantage_gles::glFrustum(left, right, bottom, top, near_val, far_val)
}

#[no_mangle]
pub unsafe extern "C" fn glFrustumf(
    left: GLfloat,
    right: GLfloat,
    bottom: GLfloat,
    top: GLfloat,
    near_val: GLfloat,
    far_val: GLfloat,
) {
    vantage_gles::glFrustumf(left, right, bottom, top, near_val, far_val)
}

#[no_mangle]
pub unsafe extern "C" fn glMultMatrixf(m: *const GLfloat) {
    vantage_gles::glMultMatrixf(m)
}

#[no_mangle]
pub unsafe extern "C" fn glLoadMatrixf(m: *const GLfloat) {
    vantage_gles::glLoadMatrixf(m)
}

#[no_mangle]
pub unsafe extern "C" fn glEnableClientState(array: GLenum) {
    vantage_gles::glEnableClientState(array)
}

#[no_mangle]
pub unsafe extern "C" fn glDisableClientState(array: GLenum) {
    vantage_gles::glDisableClientState(array)
}

#[no_mangle]
pub unsafe extern "C" fn glVertexPointer(
    size: GLint,
    type_: GLenum,
    stride: GLsizei,
    pointer: *const c_void,
) {
    vantage_gles::glVertexPointer(size, type_, stride, pointer)
}

#[no_mangle]
pub unsafe extern "C" fn glTexCoordPointer(
    size: GLint,
    type_: GLenum,
    stride: GLsizei,
    pointer: *const c_void,
) {
    vantage_gles::glTexCoordPointer(size, type_, stride, pointer)
}

#[no_mangle]
pub unsafe extern "C" fn glColorPointer(
    size: GLint,
    type_: GLenum,
    stride: GLsizei,
    pointer: *const c_void,
) {
    vantage_gles::glColorPointer(size, type_, stride, pointer)
}

#[no_mangle]
pub unsafe extern "C" fn glNormalPointer(type_: GLenum, stride: GLsizei, pointer: *const c_void) {
    vantage_gles::glNormalPointer(type_, stride, pointer)
}

#[no_mangle]
pub unsafe extern "C" fn glClientActiveTexture(texture: GLenum) {
    vantage_gles::glClientActiveTexture(texture)
}

#[no_mangle]
pub unsafe extern "C" fn glDrawArrays(mode: GLenum, first: GLint, count: GLsizei) {
    vantage_gles::glDrawArrays(mode, first, count)
}

#[no_mangle]
pub unsafe extern "C" fn glDrawElements(
    mode: GLenum,
    count: GLsizei,
    type_: GLenum,
    indices: *const c_void,
) {
    vantage_gles::glDrawElements(mode, count, type_, indices)
}

#[no_mangle]
pub unsafe extern "C" fn glBegin(mode: GLenum) {
    vantage_gles::glBegin(mode)
}

#[no_mangle]
pub unsafe extern "C" fn glEnd() {
    vantage_gles::glEnd()
}

#[no_mangle]
pub unsafe extern "C" fn glVertex3f(x: GLfloat, y: GLfloat, z: GLfloat) {
    vantage_gles::glVertex3f(x, y, z)
}

#[no_mangle]
pub unsafe extern "C" fn glVertex2f(x: GLfloat, y: GLfloat) {
    vantage_gles::glVertex2f(x, y)
}

#[no_mangle]
pub unsafe extern "C" fn glTexCoord2f(u: GLfloat, v: GLfloat) {
    vantage_gles::glTexCoord2f(u, v)
}

#[no_mangle]
pub unsafe extern "C" fn glMultiTexCoord2f(target: GLenum, s: GLfloat, t: GLfloat) {
    vantage_gles::glMultiTexCoord2f(target, s, t)
}

#[no_mangle]
pub unsafe extern "C" fn glMultiTexCoord4f(
    target: GLenum,
    s: GLfloat,
    t: GLfloat,
    r: GLfloat,
    q: GLfloat,
) {
    vantage_gles::glMultiTexCoord4f(target, s, t, r, q)
}

#[no_mangle]
pub unsafe extern "C" fn glColor4f(r: GLfloat, g: GLfloat, b: GLfloat, a: GLfloat) {
    vantage_gles::glColor4f(r, g, b, a)
}

#[no_mangle]
pub unsafe extern "C" fn glColor3f(r: GLfloat, g: GLfloat, b: GLfloat) {
    vantage_gles::glColor3f(r, g, b)
}

#[no_mangle]
pub unsafe extern "C" fn glColor4ub(r: GLubyte, g: GLubyte, b: GLubyte, a: GLubyte) {
    vantage_gles::glColor4ub(r, g, b, a)
}

#[no_mangle]
pub unsafe extern "C" fn glColor3ub(r: GLubyte, g: GLubyte, b: GLubyte) {
    vantage_gles::glColor3ub(r, g, b)
}

#[no_mangle]
pub unsafe extern "C" fn glColor4fv(v: *const GLfloat) {
    vantage_gles::glColor4fv(v)
}

#[no_mangle]
pub unsafe extern "C" fn glNormal3f(x: GLfloat, y: GLfloat, z: GLfloat) {
    vantage_gles::glNormal3f(x, y, z)
}

#[no_mangle]
pub unsafe extern "C" fn glGenLists(range: GLsizei) -> GLuint {
    vantage_gles::glGenLists(range)
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteLists(list: GLuint, range: GLsizei) {
    vantage_gles::glDeleteLists(list, range)
}

#[no_mangle]
pub unsafe extern "C" fn glNewList(list: GLuint, mode: GLenum) {
    vantage_gles::glNewList(list, mode)
}

#[no_mangle]
pub unsafe extern "C" fn glEndList() {
    vantage_gles::glEndList()
}

#[no_mangle]
pub unsafe extern "C" fn glCallList(list: GLuint) {
    vantage_gles::glCallList(list)
}

#[no_mangle]
pub unsafe extern "C" fn glCallLists(n: GLsizei, type_: GLenum, lists: *const c_void) {
    vantage_gles::glCallLists(n, type_, lists)
}

#[no_mangle]
pub unsafe extern "C" fn glIsList(list: GLuint) -> GLboolean {
    vantage_gles::glIsList(list)
}

#[no_mangle]
pub unsafe extern "C" fn glGenTextures(n: GLsizei, textures: *mut GLuint) {
    vantage_gles::glGenTextures(n, textures)
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteTextures(n: GLsizei, textures: *const GLuint) {
    vantage_gles::glDeleteTextures(n, textures)
}

#[no_mangle]
pub unsafe extern "C" fn glBindTexture(target: GLenum, texture: GLuint) {
    vantage_gles::glBindTexture(target, texture)
}

#[no_mangle]
pub unsafe extern "C" fn glTexImage2D(
    target: GLenum,
    level: GLint,
    internalformat: GLint,
    width: GLsizei,
    height: GLsizei,
    border: GLint,
    format: GLenum,
    type_: GLenum,
    pixels: *const c_void,
) {
    vantage_gles::glTexImage2D(
        target,
        level,
        internalformat,
        width,
        height,
        border,
        format,
        type_,
        pixels,
    )
}

#[no_mangle]
pub unsafe extern "C" fn glTexSubImage2D(
    target: GLenum,
    level: GLint,
    xoffset: GLint,
    yoffset: GLint,
    width: GLsizei,
    height: GLsizei,
    format: GLenum,
    type_: GLenum,
    pixels: *const c_void,
) {
    vantage_gles::glTexSubImage2D(
        target, level, xoffset, yoffset, width, height, format, type_, pixels,
    )
}

#[no_mangle]
pub unsafe extern "C" fn glTexParameteri(target: GLenum, pname: GLenum, param: GLint) {
    vantage_gles::glTexParameteri(target, pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glTexParameterf(target: GLenum, pname: GLenum, param: GLfloat) {
    vantage_gles::glTexParameterf(target, pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glTexParameteriv(target: GLenum, pname: GLenum, params: *const GLint) {
    vantage_gles::glTexParameteriv(target, pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glTexParameterfv(target: GLenum, pname: GLenum, params: *const GLfloat) {
    vantage_gles::glTexParameterfv(target, pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glActiveTexture(texture: GLenum) {
    vantage_gles::glActiveTexture(texture)
}

#[no_mangle]
pub unsafe extern "C" fn glTexImage1D(
    target: GLenum,
    level: GLint,
    internalformat: GLint,
    width: GLsizei,
    border: GLint,
    format: GLenum,
    type_: GLenum,
    pixels: *const c_void,
) {
    vantage_gles::glTexImage1D(
        target,
        level,
        internalformat,
        width,
        border,
        format,
        type_,
        pixels,
    )
}

#[no_mangle]
pub unsafe extern "C" fn glTexImage3D(
    target: GLenum,
    level: GLint,
    internalformat: GLint,
    width: GLsizei,
    height: GLsizei,
    depth: GLsizei,
    border: GLint,
    format: GLenum,
    type_: GLenum,
    pixels: *const c_void,
) {
    vantage_gles::glTexImage3D(
        target,
        level,
        internalformat,
        width,
        height,
        depth,
        border,
        format,
        type_,
        pixels,
    )
}

#[no_mangle]
pub unsafe extern "C" fn glGetTexLevelParameteri(
    target: GLenum,
    level: GLint,
    pname: GLenum,
    params: *mut GLint,
) {
    vantage_gles::glGetTexLevelParameteri(target, level, pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glTexGen(coord: GLenum, pname: GLenum, param: GLfloat) {
    vantage_gles::glTexGen(coord, pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glTexGeni(coord: GLenum, pname: GLenum, param: GLint) {
    vantage_gles::glTexGeni(coord, pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glTexEnvf(target: GLenum, pname: GLenum, param: GLfloat) {
    vantage_gles::glTexEnvf(target, pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glTexEnvi(target: GLenum, pname: GLenum, param: GLint) {
    vantage_gles::glTexEnvi(target, pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glTexEnvfv(target: GLenum, pname: GLenum, params: *const GLfloat) {
    vantage_gles::glTexEnvfv(target, pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glEnable(cap: GLenum) {
    vantage_gles::glEnable(cap)
}

#[no_mangle]
pub unsafe extern "C" fn glDisable(cap: GLenum) {
    vantage_gles::glDisable(cap)
}

#[no_mangle]
pub unsafe extern "C" fn glIsEnabled(cap: GLenum) -> GLboolean {
    vantage_gles::glIsEnabled(cap)
}

#[no_mangle]
pub unsafe extern "C" fn glAlphaFunc(func: GLenum, ref_val: GLclampf) {
    vantage_gles::glAlphaFunc(func, ref_val)
}

#[no_mangle]
pub unsafe extern "C" fn glBlendFunc(sfactor: GLenum, dfactor: GLenum) {
    vantage_gles::glBlendFunc(sfactor, dfactor)
}

#[no_mangle]
pub unsafe extern "C" fn glBlendColor(
    red: GLclampf,
    green: GLclampf,
    blue: GLclampf,
    alpha: GLclampf,
) {
    vantage_gles::glBlendColor(red, green, blue, alpha)
}

#[no_mangle]
pub unsafe extern "C" fn glBlendFuncSeparate(
    srcRGB: GLenum,
    dstRGB: GLenum,
    srcAlpha: GLenum,
    dstAlpha: GLenum,
) {
    vantage_gles::glBlendFuncSeparate(srcRGB, dstRGB, srcAlpha, dstAlpha)
}

#[no_mangle]
pub unsafe extern "C" fn glDepthFunc(func: GLenum) {
    vantage_gles::glDepthFunc(func)
}

#[no_mangle]
pub unsafe extern "C" fn glDepthMask(flag: GLboolean) {
    vantage_gles::glDepthMask(flag)
}

#[no_mangle]
pub unsafe extern "C" fn glColorMask(
    red: GLboolean,
    green: GLboolean,
    blue: GLboolean,
    alpha: GLboolean,
) {
    vantage_gles::glColorMask(red, green, blue, alpha)
}

#[no_mangle]
pub unsafe extern "C" fn glCullFace(mode: GLenum) {
    vantage_gles::glCullFace(mode)
}

#[no_mangle]
pub unsafe extern "C" fn glFrontFace(mode: GLenum) {
    vantage_gles::glFrontFace(mode)
}

#[no_mangle]
pub unsafe extern "C" fn glPolygonOffset(factor: GLfloat, units: GLfloat) {
    vantage_gles::glPolygonOffset(factor, units)
}

#[no_mangle]
pub unsafe extern "C" fn glLineWidth(width: GLfloat) {
    vantage_gles::glLineWidth(width)
}

#[no_mangle]
pub unsafe extern "C" fn glPointSize(size: GLfloat) {
    vantage_gles::glPointSize(size)
}

#[no_mangle]
pub unsafe extern "C" fn glShadeModel(mode: GLenum) {
    vantage_gles::glShadeModel(mode)
}

#[no_mangle]
pub unsafe extern "C" fn glColorMaterial(face: GLenum, mode: GLenum) {
    vantage_gles::glColorMaterial(face, mode)
}

#[no_mangle]
pub unsafe extern "C" fn glFogf(pname: GLenum, param: GLfloat) {
    vantage_gles::glFogf(pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glFogfv(pname: GLenum, params: *const GLfloat) {
    vantage_gles::glFogfv(pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glFogi(pname: GLenum, param: GLint) {
    vantage_gles::glFogi(pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glFogx(pname: GLenum, param: GLfixed) {
    vantage_gles::glFogx(pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glFogxv(pname: GLenum, params: *const GLfixed) {
    vantage_gles::glFogxv(pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glHint(target: GLenum, mode: GLenum) {
    vantage_gles::glHint(target, mode)
}

#[no_mangle]
pub unsafe extern "C" fn glDepthRangef(n: GLclampf, f: GLclampf) {
    vantage_gles::glDepthRangef(n, f)
}

#[no_mangle]
pub unsafe extern "C" fn glDepthRange(n: GLclampd, f: GLclampd) {
    vantage_gles::glDepthRange(n, f)
}

#[no_mangle]
pub unsafe extern "C" fn glDepthRangex(n: GLclampx, f: GLclampx) {
    vantage_gles::glDepthRangex(n, f)
}

#[no_mangle]
pub unsafe extern "C" fn glLightf(light: GLenum, pname: GLenum, param: GLfloat) {
    vantage_gles::glLightf(light, pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glLightfv(light: GLenum, pname: GLenum, params: *const GLfloat) {
    vantage_gles::glLightfv(light, pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glLightModelf(pname: GLenum, param: GLfloat) {
    vantage_gles::glLightModelf(pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glLightModelfv(pname: GLenum, params: *const GLfloat) {
    vantage_gles::glLightModelfv(pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glMaterialf(face: GLenum, pname: GLenum, param: GLfloat) {
    vantage_gles::glMaterialf(face, pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glMaterialfv(face: GLenum, pname: GLenum, params: *const GLfloat) {
    vantage_gles::glMaterialfv(face, pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glStencilFunc(func: GLenum, ref_val: GLint, mask: GLuint) {
    vantage_gles::glStencilFunc(func, ref_val, mask)
}

#[no_mangle]
pub unsafe extern "C" fn glStencilMask(mask: GLuint) {
    vantage_gles::glStencilMask(mask)
}

#[no_mangle]
pub unsafe extern "C" fn glStencilOp(fail: GLenum, zfail: GLenum, zpass: GLenum) {
    vantage_gles::glStencilOp(fail, zfail, zpass)
}

#[no_mangle]
pub unsafe extern "C" fn glViewport(x: GLint, y: GLint, width: GLsizei, height: GLsizei) {
    vantage_gles::glViewport(x, y, width, height)
}

#[no_mangle]
pub unsafe extern "C" fn glScissor(x: GLint, y: GLint, width: GLsizei, height: GLsizei) {
    vantage_gles::glScissor(x, y, width, height)
}

#[no_mangle]
pub unsafe extern "C" fn glClearColor(
    red: GLclampf,
    green: GLclampf,
    blue: GLclampf,
    alpha: GLclampf,
) {
    vantage_gles::glClearColor(red, green, blue, alpha)
}

#[no_mangle]
pub unsafe extern "C" fn glClearDepthf(depth: GLclampf) {
    vantage_gles::glClearDepthf(depth)
}

#[no_mangle]
pub unsafe extern "C" fn glClearDepth(depth: GLclampd) {
    vantage_gles::glClearDepth(depth)
}

#[no_mangle]
pub unsafe extern "C" fn glClearStencil(s: GLint) {
    vantage_gles::glClearStencil(s)
}

#[no_mangle]
pub unsafe extern "C" fn glClear(mask: GLbitfield) {
    vantage_gles::glClear(mask)
}

#[no_mangle]
pub unsafe extern "C" fn glPixelStorei(pname: GLenum, param: GLint) {
    vantage_gles::glPixelStorei(pname, param)
}

#[no_mangle]
pub unsafe extern "C" fn glReadPixels(
    x: GLint,
    y: GLint,
    width: GLsizei,
    height: GLsizei,
    format: GLenum,
    type_: GLenum,
    pixels: *mut c_void,
) {
    vantage_gles::glReadPixels(x, y, width, height, format, type_, pixels)
}

#[no_mangle]
pub unsafe extern "C" fn glFlush() {
    vantage_gles::glFlush()
}

#[no_mangle]
pub unsafe extern "C" fn glFinish() {
    vantage_gles::glFinish()
}

#[no_mangle]
pub unsafe extern "C" fn glGetIntegerv(pname: GLenum, params: *mut GLint) {
    vantage_gles::glGetIntegerv(pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glGetFloatv(pname: GLenum, params: *mut GLfloat) {
    vantage_gles::glGetFloatv(pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glGetBooleanv(pname: GLenum, params: *mut GLboolean) {
    vantage_gles::glGetBooleanv(pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glGetString(name: GLenum) -> *const GLubyte {
    vantage_gles::glGetString(name)
}

#[no_mangle]
pub unsafe extern "C" fn glGetError() -> GLenum {
    vantage_gles::glGetError()
}

#[no_mangle]
pub unsafe extern "C" fn glPushAttrib(mask: GLbitfield) {
    vantage_gles::glPushAttrib(mask)
}

#[no_mangle]
pub unsafe extern "C" fn glPopAttrib() {
    vantage_gles::glPopAttrib()
}

#[no_mangle]
pub unsafe extern "C" fn glPushClientAttrib(mask: GLbitfield) {
    vantage_gles::glPushClientAttrib(mask)
}

#[no_mangle]
pub unsafe extern "C" fn glPopClientAttrib() {
    vantage_gles::glPopClientAttrib()
}

#[no_mangle]
pub unsafe extern "C" fn glCreateShader(shader_type: GLenum) -> GLuint {
    vantage_gles::glCreateShader(shader_type)
}

#[no_mangle]
pub unsafe extern "C" fn glShaderSource(
    shader: GLuint,
    count: GLsizei,
    string: *const *const GLchar,
    length: *const GLint,
) {
    vantage_gles::glShaderSource(shader, count, string, length)
}

#[no_mangle]
pub unsafe extern "C" fn glCompileShader(shader: GLuint) {
    vantage_gles::glCompileShader(shader)
}

#[no_mangle]
pub unsafe extern "C" fn glGetShaderiv(shader: GLuint, pname: GLenum, params: *mut GLint) {
    vantage_gles::glGetShaderiv(shader, pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glGetShaderInfoLog(
    shader: GLuint,
    buf_size: GLsizei,
    length: *mut GLsizei,
    info_log: *mut GLchar,
) {
    vantage_gles::glGetShaderInfoLog(shader, buf_size, length, info_log)
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteShader(shader: GLuint) {
    vantage_gles::glDeleteShader(shader)
}

#[no_mangle]
pub unsafe extern "C" fn glCreateProgram() -> GLuint {
    vantage_gles::glCreateProgram()
}

#[no_mangle]
pub unsafe extern "C" fn glAttachShader(program: GLuint, shader: GLuint) {
    vantage_gles::glAttachShader(program, shader)
}

#[no_mangle]
pub unsafe extern "C" fn glDetachShader(program: GLuint, shader: GLuint) {
    vantage_gles::glDetachShader(program, shader)
}

#[no_mangle]
pub unsafe extern "C" fn glLinkProgram(program: GLuint) {
    vantage_gles::glLinkProgram(program)
}

#[no_mangle]
pub unsafe extern "C" fn glGetProgramiv(program: GLuint, pname: GLenum, params: *mut GLint) {
    vantage_gles::glGetProgramiv(program, pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glGetProgramInfoLog(
    program: GLuint,
    buf_size: GLsizei,
    length: *mut GLsizei,
    info_log: *mut GLchar,
) {
    vantage_gles::glGetProgramInfoLog(program, buf_size, length, info_log)
}

#[no_mangle]
pub unsafe extern "C" fn glUseProgram(program: GLuint) {
    vantage_gles::glUseProgram(program)
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteProgram(program: GLuint) {
    vantage_gles::glDeleteProgram(program)
}

#[no_mangle]
pub unsafe extern "C" fn glGetUniformLocation(program: GLuint, name: *const GLchar) -> GLint {
    vantage_gles::glGetUniformLocation(program, name)
}

#[no_mangle]
pub unsafe extern "C" fn glGetAttribLocation(program: GLuint, name: *const GLchar) -> GLint {
    vantage_gles::glGetAttribLocation(program, name)
}

#[no_mangle]
pub unsafe extern "C" fn glUniform1f(location: GLint, v0: GLfloat) {
    vantage_gles::glUniform1f(location, v0)
}

#[no_mangle]
pub unsafe extern "C" fn glUniform2f(location: GLint, v0: GLfloat, v1: GLfloat) {
    vantage_gles::glUniform2f(location, v0, v1)
}

#[no_mangle]
pub unsafe extern "C" fn glUniform3f(location: GLint, v0: GLfloat, v1: GLfloat, v2: GLfloat) {
    vantage_gles::glUniform3f(location, v0, v1, v2)
}

#[no_mangle]
pub unsafe extern "C" fn glUniform4f(
    location: GLint,
    v0: GLfloat,
    v1: GLfloat,
    v2: GLfloat,
    v3: GLfloat,
) {
    vantage_gles::glUniform4f(location, v0, v1, v2, v3)
}

#[no_mangle]
pub unsafe extern "C" fn glUniform1i(location: GLint, v0: GLint) {
    vantage_gles::glUniform1i(location, v0)
}

#[no_mangle]
pub unsafe extern "C" fn glUniform2i(location: GLint, v0: GLint, v1: GLint) {
    vantage_gles::glUniform2i(location, v0, v1)
}

#[no_mangle]
pub unsafe extern "C" fn glUniform3i(location: GLint, v0: GLint, v1: GLint, v2: GLint) {
    vantage_gles::glUniform3i(location, v0, v1, v2)
}

#[no_mangle]
pub unsafe extern "C" fn glUniform4i(location: GLint, v0: GLint, v1: GLint, v2: GLint, v3: GLint) {
    vantage_gles::glUniform4i(location, v0, v1, v2, v3)
}

#[no_mangle]
pub unsafe extern "C" fn glUniformMatrix4fv(
    location: GLint,
    count: GLsizei,
    transpose: GLboolean,
    value: *const GLfloat,
) {
    vantage_gles::glUniformMatrix4fv(location, count, transpose, value)
}

#[no_mangle]
pub unsafe extern "C" fn glGenBuffers(n: GLsizei, buffers: *mut GLuint) {
    vantage_gles::glGenBuffers(n, buffers)
}

#[no_mangle]
pub unsafe extern "C" fn glGenBuffersARB(n: GLsizei, buffers: *mut GLuint) {
    vantage_gles::glGenBuffersARB(n, buffers)
}

#[no_mangle]
pub unsafe extern "C" fn glBindBuffer(target: GLenum, buffer: GLuint) {
    vantage_gles::glBindBuffer(target, buffer)
}

#[no_mangle]
pub unsafe extern "C" fn glBindBufferARB(target: GLenum, buffer: GLuint) {
    vantage_gles::glBindBufferARB(target, buffer)
}

#[no_mangle]
pub unsafe extern "C" fn glBufferData(
    target: GLenum,
    size: GLsizeiptr,
    data: *const c_void,
    usage: GLenum,
) {
    vantage_gles::glBufferData(target, size, data, usage)
}

#[no_mangle]
pub unsafe extern "C" fn glBufferDataARB(
    target: GLenum,
    size: GLsizeiptr,
    data: *const c_void,
    usage: GLenum,
) {
    vantage_gles::glBufferDataARB(target, size, data, usage)
}

#[no_mangle]
pub unsafe extern "C" fn glBufferSubData(
    target: GLenum,
    offset: GLintptr,
    size: GLsizeiptr,
    data: *const c_void,
) {
    vantage_gles::glBufferSubData(target, offset, size, data)
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteBuffers(n: GLsizei, buffers: *const GLuint) {
    vantage_gles::glDeleteBuffers(n, buffers)
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteBuffersARB(n: GLsizei, buffers: *const GLuint) {
    vantage_gles::glDeleteBuffersARB(n, buffers)
}

#[no_mangle]
pub unsafe extern "C" fn glVertexAttribPointer(
    index: GLuint,
    size: GLint,
    type_: GLenum,
    normalized: GLboolean,
    stride: GLsizei,
    pointer: *const c_void,
) {
    vantage_gles::glVertexAttribPointer(index, size, type_, normalized, stride, pointer)
}

#[no_mangle]
pub unsafe extern "C" fn glEnableVertexAttribArray(index: GLuint) {
    vantage_gles::glEnableVertexAttribArray(index)
}

#[no_mangle]
pub unsafe extern "C" fn glDisableVertexAttribArray(index: GLuint) {
    vantage_gles::glDisableVertexAttribArray(index)
}

#[no_mangle]
pub unsafe extern "C" fn glGenFramebuffers(n: GLsizei, framebuffers: *mut GLuint) {
    vantage_gles::glGenFramebuffers(n, framebuffers)
}

#[no_mangle]
pub unsafe extern "C" fn glBindFramebuffer(target: GLenum, framebuffer: GLuint) {
    vantage_gles::glBindFramebuffer(target, framebuffer)
}

#[no_mangle]
pub unsafe extern "C" fn glFramebufferTexture2D(
    target: GLenum,
    attachment: GLenum,
    textarget: GLenum,
    texture: GLuint,
    level: GLint,
) {
    vantage_gles::glFramebufferTexture2D(target, attachment, textarget, texture, level)
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteFramebuffers(n: GLsizei, framebuffers: *const GLuint) {
    vantage_gles::glDeleteFramebuffers(n, framebuffers)
}

#[no_mangle]
pub unsafe extern "C" fn glCheckFramebufferStatus(target: GLenum) -> GLenum {
    vantage_gles::glCheckFramebufferStatus(target)
}

#[no_mangle]
pub unsafe extern "C" fn glGenRenderbuffers(n: GLsizei, renderbuffers: *mut GLuint) {
    vantage_gles::glGenRenderbuffers(n, renderbuffers)
}

#[no_mangle]
pub unsafe extern "C" fn glBindRenderbuffer(target: GLenum, renderbuffer: GLuint) {
    vantage_gles::glBindRenderbuffer(target, renderbuffer)
}

#[no_mangle]
pub unsafe extern "C" fn glRenderbufferStorage(
    target: GLenum,
    internalformat: GLenum,
    width: GLsizei,
    height: GLsizei,
) {
    vantage_gles::glRenderbufferStorage(target, internalformat, width, height)
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteRenderbuffers(n: GLsizei, renderbuffers: *const GLuint) {
    vantage_gles::glDeleteRenderbuffers(n, renderbuffers)
}

#[no_mangle]
pub unsafe extern "C" fn glGenerateMipmap(target: GLenum) {
    vantage_gles::glGenerateMipmap(target)
}

#[no_mangle]
pub unsafe extern "C" fn glGenQueries(n: GLsizei, ids: *mut GLuint) {
    vantage_gles::glGenQueries(n, ids)
}

#[no_mangle]
pub unsafe extern "C" fn glGenQueriesARB(n: GLsizei, ids: *mut GLuint) {
    vantage_gles::glGenQueriesARB(n, ids)
}

#[no_mangle]
pub unsafe extern "C" fn glBeginQuery(target: GLenum, id: GLuint) {
    vantage_gles::glBeginQuery(target, id)
}

#[no_mangle]
pub unsafe extern "C" fn glBeginQueryARB(target: GLenum, id: GLuint) {
    vantage_gles::glBeginQueryARB(target, id)
}

#[no_mangle]
pub unsafe extern "C" fn glEndQuery(target: GLenum) {
    vantage_gles::glEndQuery(target)
}

#[no_mangle]
pub unsafe extern "C" fn glEndQueryARB(target: GLenum) {
    vantage_gles::glEndQueryARB(target)
}

#[no_mangle]
pub unsafe extern "C" fn glGetQueryObjectuiv(id: GLuint, pname: GLenum, params: *mut GLuint) {
    vantage_gles::glGetQueryObjectuiv(id, pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glGetQueryObjectuivARB(id: GLuint, pname: GLenum, params: *mut GLuint) {
    vantage_gles::glGetQueryObjectuivARB(id, pname, params)
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteQueries(n: GLsizei, ids: *const GLuint) {
    vantage_gles::glDeleteQueries(n, ids)
}

#[no_mangle]
pub unsafe extern "C" fn glDeleteQueriesARB(n: GLsizei, ids: *const GLuint) {
    vantage_gles::glDeleteQueriesARB(n, ids)
}
