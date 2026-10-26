#pragma once
// Minimal hidden-window OpenGL context for the tests (separate translation unit: FFGL.h defines NOUSER, which hides the
// window APIs needed here).
#include <string>

//! Creates a window + WGL context, makes it current and initialises GLEW. Returns false (with a reason in `info`) if that
//! is not possible; otherwise `info` holds the GL_VERSION / GL_RENDERER strings.
bool CreateTestGlContext( std::string& info );
void DestroyTestGlContext();
