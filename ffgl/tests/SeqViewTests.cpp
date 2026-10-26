// Tests for SeqView: instantiates the real plugin class (the code that runs in Resolume), gives it a hidden
// OpenGL context and a host-side FBO, drives parameters like a host would, checks pixels and saves PNG previews.
// Usage: SeqViewTests.exe [output-directory]
#include "../SeqView.h"
#include "../SeqViewParams.h"
#include "GlContext.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

// On a dual-GPU (Optimus/AMD switchable-graphics) laptop, Windows defaults a plain console app to the integrated
// GPU, while a real-time video host like Resolume typically runs on the discrete one. These exported symbols are
// the standard way an application opts into the discrete GPU on such a machine - without them, this test exercises
// a different GPU/driver than Resolume actually uses, which can hide real bugs (a shader that compiles and runs
// fine on an integrated Intel driver can be rejected outright, or behave differently, on a stricter NVIDIA driver).
extern "C"
{
	__declspec( dllexport ) unsigned long NvOptimusEnablement                  = 0x00000001;
	__declspec( dllexport ) int           AmdPowerXpressRequestHighPerformance = 1;
}

// ---------------------------------------------------------------------------------------------------------------------
static int g_failures = 0, g_checks = 0;
#define CHECK( cond, ... )                                                \
	do                                                                    \
	{                                                                     \
		++g_checks;                                                       \
		if( !( cond ) )                                                   \
		{                                                                 \
			++g_failures;                                                 \
			printf( "  FAIL %s:%d  %s  -- ", __FILE__, __LINE__, #cond ); \
			printf( __VA_ARGS__ );                                        \
			printf( "\n" );                                               \
		}                                                                 \
	} while( 0 )

// ---------------------------------------------------------------------------------------------------------------------
// Minimal PNG writer (stored/uncompressed deflate), same scheme as the other plugins' tests in this repo.
static uint32_t Crc32( const uint8_t* d, size_t n )
{
	static uint32_t table[ 256 ];
	static bool init = false;
	if( !init )
	{
		for( uint32_t i = 0; i < 256; ++i )
		{
			uint32_t c = i;
			for( int k = 0; k < 8; ++k )
				c = ( c & 1 ) ? 0xEDB88320u ^ ( c >> 1 ) : c >> 1;
			table[ i ] = c;
		}
		init = true;
	}
	uint32_t crc = ~0u;
	for( size_t i = 0; i < n; ++i )
		crc = table[ ( crc ^ d[ i ] ) & 0xFF ] ^ ( crc >> 8 );
	return ~crc;
}
static void PutU32( std::vector< uint8_t >& v, uint32_t x )
{
	for( int s = 24; s >= 0; s -= 8 )
		v.push_back( static_cast< uint8_t >( x >> s ) );
}
static void PngChunk( std::vector< uint8_t >& out, const char* type, const std::vector< uint8_t >& data )
{
	PutU32( out, static_cast< uint32_t >( data.size() ) );
	std::vector< uint8_t > body( type, type + 4 );
	body.insert( body.end(), data.begin(), data.end() );
	out.insert( out.end(), body.begin(), body.end() );
	PutU32( out, Crc32( body.data(), body.size() ) );
}
static bool WritePng( const std::filesystem::path& path, const uint8_t* rgba, int w, int h )
{
	std::vector< uint8_t > raw;
	raw.reserve( static_cast< size_t >( w ) * h * 4 + h );
	for( int y = 0; y < h; ++y )
	{
		raw.push_back( 0 );
		raw.insert( raw.end(), rgba + static_cast< size_t >( y ) * w * 4, rgba + static_cast< size_t >( y + 1 ) * w * 4 );
	}
	std::vector< uint8_t > z = { 0x78, 0x01 };
	for( size_t pos = 0; pos < raw.size(); )
	{
		const size_t n = std::min< size_t >( 65535, raw.size() - pos );
		z.push_back( static_cast< uint8_t >( pos + n == raw.size() ? 1 : 0 ) );
		z.push_back( static_cast< uint8_t >( n & 0xFF ) );
		z.push_back( static_cast< uint8_t >( n >> 8 ) );
		z.push_back( static_cast< uint8_t >( ~n & 0xFF ) );
		z.push_back( static_cast< uint8_t >( ( ~n >> 8 ) & 0xFF ) );
		z.insert( z.end(), raw.begin() + pos, raw.begin() + pos + n );
		pos += n;
	}
	uint32_t a = 1, b = 0;
	for( uint8_t c : raw )
	{
		a = ( a + c ) % 65521;
		b = ( b + a ) % 65521;
	}
	PutU32( z, ( b << 16 ) | a );
	std::vector< uint8_t > png = { 0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A };
	std::vector< uint8_t > ihdr;
	PutU32( ihdr, w );
	PutU32( ihdr, h );
	ihdr.insert( ihdr.end(), { 8, 6, 0, 0, 0 } );//8 bit RGBA
	PngChunk( png, "IHDR", ihdr );
	PngChunk( png, "IDAT", z );
	PngChunk( png, "IEND", {} );
	std::ofstream f( path, std::ios::binary );
	f.write( reinterpret_cast< const char* >( png.data() ), static_cast< std::streamsize >( png.size() ) );
	return f.good();
}

// ---------------------------------------------------------------------------------------------------------------------
namespace
{
struct Frame
{
	int w = 0, h = 0;
	std::vector< uint8_t > rgba; //top-down, premultiplied
	size_t lit = 0;              //pixels with alpha > 0
	uint8_t At( int x, int y, int c ) const { return rgba[ ( static_cast< size_t >( y ) * w + x ) * 4 + c ]; }
};

//! Owns the host-side FBO the plugin renders into. Can be recreated at a different size to simulate a composition
//! resize - the whole point of this harness is to prove the plugin adapts, rather than caching its first size.
struct Host
{
	GLuint fbo = 0, tex = 0;
	int w = 0, h = 0;
	void Create( int width, int height )
	{
		Destroy();
		w = width;
		h = height;
		glGenTextures( 1, &tex );
		glBindTexture( GL_TEXTURE_2D, tex );
		glTexImage2D( GL_TEXTURE_2D, 0, GL_RGBA8, w, h, 0, GL_RGBA, GL_UNSIGNED_BYTE, nullptr );
		glBindTexture( GL_TEXTURE_2D, 0 );
		glGenFramebuffers( 1, &fbo );
		glBindFramebuffer( GL_FRAMEBUFFER, fbo );
		glFramebufferTexture2D( GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, tex, 0 );
		glBindFramebuffer( GL_FRAMEBUFFER, 0 );
	}
	void Destroy()
	{
		if( fbo )
			glDeleteFramebuffers( 1, &fbo );
		if( tex )
			glDeleteTextures( 1, &tex );
		fbo = tex = 0;
	}
	Frame Step( SeqView& plugin, bool read )
	{
		glBindFramebuffer( GL_FRAMEBUFFER, fbo );
		glViewport( 0, 0, w, h );
		glClearColor( 0.2f, 0.2f, 0.2f, 0.3f ); //deliberately not black/zero: the plugin must overwrite every pixel it draws
		glClear( GL_COLOR_BUFFER_BIT );
		ProcessOpenGLStruct pgl = {};
		pgl.HostFBO             = fbo;
		plugin.ProcessOpenGL( &pgl );
		Frame f;
		if( read )
		{
			f.w = w;
			f.h = h;
			std::vector< uint8_t > bottomUp( static_cast< size_t >( w ) * h * 4 );
			glReadPixels( 0, 0, w, h, GL_RGBA, GL_UNSIGNED_BYTE, bottomUp.data() );
			f.rgba.resize( bottomUp.size() );
			for( int y = 0; y < h; ++y )
				std::copy( bottomUp.begin() + static_cast< size_t >( h - 1 - y ) * w * 4, bottomUp.begin() + static_cast< size_t >( h - y ) * w * 4, f.rgba.begin() + static_cast< size_t >( y ) * w * 4 );
			for( size_t i = 3; i < f.rgba.size(); i += 4 )
				f.lit += f.rgba[ i ] > 0;
		}
		glBindFramebuffer( GL_FRAMEBUFFER, 0 );
		return f;
	}
};

void SavePreview( const std::filesystem::path& path, const Frame& f )
{
	std::vector< uint8_t > out( f.rgba );
	for( size_t i = 3; i < out.size(); i += 4 )
		out[ i ] = 255;
	WritePng( path, out.data(), f.w, f.h );
}

GLenum DrainGlErrors()
{
	GLenum first = GL_NO_ERROR;
	for( GLenum e = glGetError(); e != GL_NO_ERROR; e = glGetError() )
		if( first == GL_NO_ERROR )
			first = e;
	return first;
}
}// namespace

// ---------------------------------------------------------------------------------------------------------------------
int main( int argc, char** argv )
{
	std::filesystem::path outDir = argc >= 2 ? std::filesystem::path( argv[ 1 ] ) : std::filesystem::path( "test_output" );
	outDir                       = std::filesystem::absolute( outDir );
	std::filesystem::create_directories( outDir );

	std::string glInfo;
	if( !CreateTestGlContext( glInfo ) )
	{
		printf( "GL init failed: %s\n", glInfo.c_str() );
		return 1;
	}
	printf( "GL: %s\n", glInfo.c_str() );
	DrainGlErrors();

	SeqView plugin;
	FFGLViewportStruct vp = { 0, 0, 640, 360 };
	CHECK( plugin.InitGL( &vp ) == FF_SUCCESS, "InitGL" );
	CHECK( DrainGlErrors() == GL_NO_ERROR, "no GL error after InitGL" );

	Host host;
	host.Create( 640, 360 );

	// ---- no sequence: opaque, every pixel overwritten ----
	Frame empty = host.Step( plugin, true );
	CHECK( empty.lit == static_cast< size_t >( empty.w ) * empty.h, "fills every pixel (%zu / %d)", empty.lit, empty.w * empty.h );
	CHECK( empty.At( 10, 10, 3 ) == 255, "opaque (alpha %d)", empty.At( 10, 10, 3 ) );
	SavePreview( outDir / "01_no_sequence.png", empty );

	// ---- the Sequence text parameter round-trips and visibly changes the output ----
	CHECK( plugin.SetTextParameter( PT_SEQUENCE, "Drop A" ) == FF_SUCCESS, "SetTextParameter(Sequence)" );
	CHECK( std::string( plugin.GetTextParameter( PT_SEQUENCE ) ) == "Drop A", "Sequence round-trips (got '%s')", plugin.GetTextParameter( PT_SEQUENCE ) );
	Frame dropA = host.Step( plugin, true );
	const auto differs = []( const Frame& a, const Frame& b ) {
		for( int c = 0; c < 3; ++c )
			if( std::abs( a.At( a.w / 2, a.h / 2, c ) - b.At( b.w / 2, b.h / 2, c ) ) > 20 )
				return true;
		return false;
	};
	CHECK( differs( empty, dropA ), "setting a sequence changes the rendered colour" );
	SavePreview( outDir / "02_drop_a.png", dropA );
	plugin.SetTextParameter( PT_SEQUENCE, "Intro" );
	Frame intro = host.Step( plugin, true );
	CHECK( differs( dropA, intro ), "another sequence name renders differently" );
	plugin.SetTextParameter( PT_SEQUENCE, "Drop A" );
	CHECK( !differs( dropA, host.Step( plugin, true ) ), "same name, same colour (stable)" );
	CHECK( plugin.SetTextParameter( PT_BUILD, "x" ) == FF_SUCCESS, "Build is accepted (display only)" );
	CHECK( plugin.SetTextParameter( 99, "x" ) == FF_FAIL, "unknown text index fails" );

	// ---- resize: the new size is used immediately (no cached viewport) ----
	FFGLViewportStruct bigVp = { 0, 0, 1920, 1080 };
	plugin.Resize( &bigVp );
	host.Create( 1920, 1080 );
	Frame resized = host.Step( plugin, true );
	CHECK( resized.lit == static_cast< size_t >( resized.w ) * resized.h, "after a resize every pixel of the new size is drawn (%zu / %d)",
	       resized.lit, resized.w * resized.h );

	CHECK( DrainGlErrors() == GL_NO_ERROR, "no GL error after rendering" );
	CHECK( plugin.DeInitGL() == FF_SUCCESS, "DeInitGL" );
	host.Destroy();
	DestroyTestGlContext();

	printf( "\n%d checks, %d failed\n", g_checks, g_failures );
	return g_failures ? 1 : 0;
}
