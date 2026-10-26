#include "GlContext.h"

#include <windows.h>
#include <GL/glew.h>

static HWND g_window = NULL;
static HDC g_dc      = NULL;
static HGLRC g_rc    = NULL;

bool CreateTestGlContext( std::string& info )
{
	WNDCLASSA wc     = {};
	wc.lpfnWndProc   = DefWindowProcA;
	wc.hInstance     = GetModuleHandleA( NULL );
	wc.lpszClassName = "SeqViewTestGl";
	RegisterClassA( &wc );
	g_window = CreateWindowA( wc.lpszClassName, "SeqViewTestGl", WS_OVERLAPPEDWINDOW, 0, 0, 64, 64, NULL, NULL, wc.hInstance, NULL );
	if( !g_window )
	{
		info = "CreateWindow failed";
		return false;
	}
	g_dc = GetDC( g_window );
	PIXELFORMATDESCRIPTOR pfd = {};
	pfd.nSize                 = sizeof( pfd );
	pfd.nVersion              = 1;
	pfd.dwFlags               = PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER;
	pfd.iPixelType            = PFD_TYPE_RGBA;
	pfd.cColorBits            = 32;
	pfd.cDepthBits            = 24;
	const int format          = ChoosePixelFormat( g_dc, &pfd );
	if( format == 0 || !SetPixelFormat( g_dc, format, &pfd ) )
	{
		info = "no pixel format";
		return false;
	}
	g_rc = wglCreateContext( g_dc );
	if( !g_rc || !wglMakeCurrent( g_dc, g_rc ) )
	{
		info = "wglCreateContext/wglMakeCurrent failed";
		return false;
	}
	glewExperimental = GL_TRUE;
	const GLenum err = glewInit();
	if( err != GLEW_OK )
	{
		info = std::string( "glewInit failed: " ) + reinterpret_cast< const char* >( glewGetErrorString( err ) );
		return false;
	}
	glGetError();//glewInit may leave a harmless GL_INVALID_ENUM behind on core profiles
	info = std::string( reinterpret_cast< const char* >( glGetString( GL_VERSION ) ) ) + " | " + reinterpret_cast< const char* >( glGetString( GL_RENDERER ) );
	if( !GLEW_VERSION_4_1 )
	{
		info += " (OpenGL 4.1 required)";
		return false;
	}
	return true;
}

void DestroyTestGlContext()
{
	if( g_rc )
	{
		wglMakeCurrent( NULL, NULL );
		wglDeleteContext( g_rc );
	}
	if( g_window )
	{
		if( g_dc )
			ReleaseDC( g_window, g_dc );
		DestroyWindow( g_window );
	}
	g_rc = NULL;
	g_dc = NULL;
	g_window = NULL;
}
