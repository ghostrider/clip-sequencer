// Tests for SeqView: instantiates the real plugin class (the code that runs in Resolume), gives it a hidden
// OpenGL context and a host-side FBO, drives parameters like a host would, checks pixels and saves PNG previews.
// Usage: SeqViewTests.exe [output-directory]
#include "../SeqView.h"
#include "../SeqViewParams.h"
#include "GlContext.h"
#include "MiniHttp.h"

#include <algorithm>
#include <chrono>
#include <thread>
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
static std::vector< uint8_t > EncodePng( const uint8_t* rgba, int w, int h )
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
	return png;
}
static bool WritePng( const std::filesystem::path& path, const uint8_t* rgba, int w, int h )
{
	const std::vector< uint8_t > png = EncodePng( rgba, w, h );
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
namespace
{
constexpr int kRed[ 3 ]    = { 255, 0, 0 };
constexpr int kAccent[ 3 ] = { 0xF2, 0xA3, 0x3A };
constexpr int kErr[ 3 ]    = { 0xE5, 0x48, 0x4D };
constexpr int kBg[ 3 ]     = { 0x0E, 0x0F, 0x11 };

bool Near( const Frame& f, int x, int y, const int rgb[ 3 ], int tol = 12 )
{
	for( int c = 0; c < 3; ++c )
		if( std::abs( f.At( x, y, c ) - rgb[ c ] ) > tol )
			return false;
	return true;
}

size_t Count( const Frame& f, int x0, int y0, int x1, int y1, const int rgb[ 3 ], int tol = 12 )
{
	size_t n = 0;
	for( int y = std::max( y0, 0 ); y < std::min( y1, f.h ); ++y )
		for( int x = std::max( x0, 0 ); x < std::min( x1, f.w ); ++x )
			n += Near( f, x, y, rgb, tol );
	return n;
}

//! Leftmost x of an accent-coloured column that spans most of the given rows (the playhead), or -1.
int PlayheadX( const Frame& f, int y0, int y1 )
{
	for( int x = 0; x < f.w; ++x )
	{
		int hits = 0;
		for( int y = y0; y < y1; ++y )
			hits += Near( f, x, y, kAccent, 20 );
		if( hits > ( y1 - y0 ) * 9 / 10 )
			return x;
	}
	return -1;
}

//! The same two-bar view in SEQVIEW 1 (eighths) or, with `sixteenths`, SEQVIEW 2 (sixteenths).
std::string Response( bool withLayout, uint64_t pos, bool sixteenths = false )
{
	const int k = sixteenths ? 2 : 1;
	std::string s = sixteenths ? "SEQVIEW 2\nstatus ok\nrev abc\n" : "SEQVIEW 1\nstatus ok\nrev abc\n";
	if( withLayout )
		s += "layout\nname Loop\ndeck Act 1\npass " + std::to_string( 16 * k ) + "\n"
		     "lane ok loop_to_end BG\n"
		     "step clip " + std::to_string( 8 * k ) + " 7 v1 Tunnel Red\n"
		     "step rest " + std::to_string( 8 * k ) + " 0 - rest\n"
		     "lane ok hold FX\n"
		     "step unresolved " + std::to_string( 16 * k ) + " 0 - Glitch Bars\n"
		     "endlayout\n";
	s += "run 1 " + std::to_string( pos ) + " 192 2 4 1.3\nlanestate 0 playing 0\nlanestate 1 playing 0\nclock 0 0\nend\n";
	return s;
}

std::vector< uint8_t > SolidRgba( int w, int h, uint8_t r, uint8_t g, uint8_t b )
{
	std::vector< uint8_t > px( static_cast< size_t >( w ) * h * 4 );
	for( size_t i = 0; i < px.size(); i += 4 )
		px[ i ] = r, px[ i + 1 ] = g, px[ i + 2 ] = b, px[ i + 3 ] = 255;
	return px;
}

void ParserTests()
{
	using namespace seqview;
	View v;
	CHECK( ParseView( Response( true, 48 ), v ), "full response parses" );
	CHECK( v.status == Status::Ok && v.rev == "abc" && v.hasLayout, "status/rev/layout" );
	CHECK( v.layout.name == "Loop" && v.layout.deck == "Act 1" && v.layout.passUnits == 16 && v.layout.unitsPerBar == 8, "header fields (deck with a space)" );
	CHECK( v.layout.lanes.size() == 2 && v.layout.lanes[ 0 ].layer == "BG" && v.layout.lanes[ 1 ].end == "hold", "lanes" );
	const Step& s0 = v.layout.lanes[ 0 ].steps[ 0 ];
	CHECK( s0.kind == StepKind::Clip && s0.units == 8 && s0.clipId == 7 && s0.thumb == "v1" && s0.label == "Tunnel Red", "clip step" );
	CHECK( v.layout.lanes[ 0 ].steps[ 1 ].kind == StepKind::Rest && v.layout.lanes[ 0 ].steps[ 1 ].thumb.empty(), "rest step" );
	CHECK( v.layout.lanes[ 1 ].steps[ 0 ].kind == StepKind::Unresolved && v.layout.lanes[ 1 ].steps[ 0 ].label == "Glitch Bars", "unresolved step" );
	CHECK( v.runtime.running && v.runtime.pos == 48 && v.runtime.passTicks == 192 && v.runtime.pass == 2 && v.runtime.passes == 4 &&
	           v.runtime.barBeat == "1.3",
	       "runtime" );
	CHECK( v.runtime.lanes.size() == 2 && v.runtime.lanes[ 0 ].state == "playing" && v.runtime.lanes[ 0 ].step == 0, "lane states" );

	// Unchanged layout is not resent: the old one stays, the runtime updates.
	CHECK( ParseView( Response( false, 96 ), v ), "runtime-only response parses" );
	CHECK( v.hasLayout && v.layout.lanes.size() == 2 && v.runtime.pos == 96, "layout kept, runtime updated" );
	CHECK( ParseView( "SEQVIEW 1\nstatus ok\nrev abc\nrun 0\nclock 1 128.00\nend\n", v ), "stopped" );
	CHECK( !v.runtime.running && v.runtime.clockRunning && std::abs( v.runtime.bpm - 128.0 ) < 1e-9, "stopped runtime + clock" );

	CHECK( ParseView( "SEQVIEW 1\nstatus not_found\nend\n", v ) && v.status == Status::NotFound && !v.hasLayout, "not found clears" );
	View bad;
	CHECK( !ParseView( "<html>nope</html>", bad ) && bad.status == Status::BadResponse, "foreign response rejected" );
	CHECK( !ParseView( "SEQVIEW 1\nstatus ok\nlayout\nname x\n", bad ), "truncated response rejected" );
	// SEQVIEW 2: lengths in sixteenths; the runtime-only update keeps the layout and its unit.
	View v2;
	CHECK( ParseView( Response( true, 0, true ), v2 ), "SEQVIEW 2 parses" );
	CHECK( v2.layout.unitsPerBar == 16 && v2.layout.passUnits == 32 && v2.layout.lanes[ 0 ].steps[ 0 ].units == 16, "SEQVIEW 2 units" );
	CHECK( ParseView( Response( false, 96, true ), v2 ) && v2.layout.unitsPerBar == 16 && v2.runtime.pos == 96, "SEQVIEW 2 runtime update" );
	CHECK( !ParseView( "SEQVIEW 3\nstatus ok\nend\n", bad ), "unknown SEQVIEW version rejected" );
	CHECK( UrlEncode( "\xC2\xA7 a1&b" ) == "%C2%A7%20a1%26b", "url encoding (%s)", UrlEncode( "\xC2\xA7 a1&b" ).c_str() );
}
}// namespace

// ---------------------------------------------------------------------------------------------------------------------
int main( int argc, char** argv )
{
	std::filesystem::path outDir = argc >= 2 ? std::filesystem::path( argv[ 1 ] ) : std::filesystem::path( "test_output" );
	outDir                       = std::filesystem::absolute( outDir );
	std::filesystem::create_directories( outDir );

	printf( "Parser...\n" );
	ParserTests();

	std::string glInfo;
	if( !CreateTestGlContext( glInfo ) )
	{
		printf( "GL init failed: %s\n", glInfo.c_str() );
		return 1;
	}
	printf( "GL: %s\n", glInfo.c_str() );
	DrainGlErrors();

	const int W = 1280, H = 720;
	Host host;
	host.Create( W, H );

	// ---- no sequence assigned: opaque background, a message in the middle ----
	{
		printf( "No sequence...\n" );
		SeqView plugin;
		FFGLViewportStruct vp = { 0, 0, W, H };
		CHECK( plugin.InitGL( &vp ) == FF_SUCCESS, "InitGL" );
		CHECK( plugin.FontReady(), "font atlas baked from the Windows fonts folder" );
		CHECK( DrainGlErrors() == GL_NO_ERROR, "no GL error after InitGL" );
		Frame f = host.Step( plugin, true );
		CHECK( f.lit == static_cast< size_t >( W ) * H, "every pixel drawn" );
		CHECK( f.At( 5, 5, 3 ) == 255 && Near( f, 5, 5, kBg, 2 ), "opaque background in the UI's bg colour" );
		size_t bright = 0;
		for( int x = 0; x < W; ++x )
			for( int y = H / 2 - 80; y < H / 2 + 60; ++y )
				bright += f.At( x, y, 0 ) > 120;
		CHECK( bright > 500, "a message is written in the middle (%zu bright pixels)", bright );
		SavePreview( outDir / "01_no_sequence.png", f );
		CHECK( plugin.DeInitGL() == FF_SUCCESS, "DeInitGL" );
	}

	// ---- an injected view: thumbnails, unresolved step, playhead position ----
	{
		printf( "Injected view...\n" );
		SeqView plugin;
		FFGLViewportStruct vp = { 0, 0, W, H };
		plugin.InitGL( &vp );
		plugin.SetTextParameter( PT_SEQUENCE, "Loop" );
		CHECK( std::string( plugin.GetTextParameter( PT_SEQUENCE ) ) == "Loop", "Sequence round-trips" );
		CHECK( std::string( plugin.GetTextParameter( PT_SERVER ) ) == kDefaultServer, "Server default" );

		seqview::View v;
		CHECK( seqview::ParseView( Response( true, 48 ), v ), "fixture parses" );
		seqview::Thumbnail red;
		red.clipId  = 7;
		red.version = "v1";
		red.width   = 128;
		red.height  = 72;
		red.rgba    = SolidRgba( 128, 72, 255, 0, 0 );
		plugin.InjectForTest( v, { red } );
		Frame a = host.Step( plugin, true );
		CHECK( plugin.TextureCount() == 1, "thumbnail uploaded" );
		CHECK( plugin.CurrentView().status == seqview::Status::Ok, "view taken over" );
		SavePreview( outDir / "02_view_pos4.png", a );

		// Lane rows: the area below the header. Red thumbnail in lane 1, red error frame in lane 2.
		const size_t red1 = Count( a, 0, 0, W, H / 2 + 60, kRed, 10 );
		CHECK( red1 > 5000, "thumbnail drawn in its step (%zu red pixels)", red1 );
		const size_t errFrame = Count( a, 0, H / 2, W, H, kErr, 10 );
		CHECK( errFrame > 500, "unresolved step framed in the error colour (%zu)", errFrame );
		const int px4 = PlayheadX( a, 160, H - 60 );
		CHECK( px4 > 0, "playhead found at position 4/16" );

		// Same view, position 96 ticks = 8 eighths: the playhead moves right by 4/16 of the timeline.
		CHECK( seqview::ParseView( Response( false, 96 ), v ), "runtime update parses" );
		plugin.InjectForTest( v, {} );
		Frame b = host.Step( plugin, true );
		SavePreview( outDir / "03_view_pos8.png", b );
		const int px8 = PlayheadX( b, 160, H - 60 );
		// Timeline geometry (SeqViewRender.cpp): margin 32u, head min(260u, 0.18 W), u = H / 1080.
		const float u        = H / 1080.0f;
		const float timeline = ( W - 32 * u ) - ( 32 * u + std::min( 260 * u, W * 0.18f ) );
		CHECK( px8 > px4 && std::abs( ( px8 - px4 ) - timeline * 4 / 16 ) < 3, "playhead advanced by a quarter (%d -> %d, expected %.1f)",
		       px4, px8, timeline * 4 / 16 );

		// The same view as SEQVIEW 2 (sixteenths): identical geometry, the playhead at the same place.
		seqview::View v16;
		CHECK( seqview::ParseView( Response( true, 96, true ), v16 ), "SEQVIEW 2 fixture parses" );
		plugin.InjectForTest( v16, { red } );
		Frame c = host.Step( plugin, true );
		SavePreview( outDir / "03b_view_pos8_sixteenths.png", c );
		const int px8s = PlayheadX( c, 160, H - 60 );
		CHECK( std::abs( px8s - px8 ) <= 1, "SEQVIEW 2 playhead matches SEQVIEW 1 (%d vs %d)", px8s, px8 );
		// Compared with the SEQVIEW 1 frame at the same position (b): the blocks are the same size.
		const size_t red16 = Count( c, 0, 0, W, H / 2 + 60, kRed, 10 ), red8 = Count( b, 0, 0, W, H / 2 + 60, kRed, 10 );
		CHECK( red8 > 5000 && red16 == red8, "SEQVIEW 2 thumbnail block has the same size (%zu vs %zu)", red16, red8 );
		// A 1/16 step: 1 of 32 units wide; the playhead at 6 ticks sits one sixteenth in.
		CHECK( seqview::ParseView( "SEQVIEW 2\nstatus ok\nrev s1\nlayout\nname S\ndeck D\npass 32\nlane ok hold BG\n"
		                           "step clip 1 7 v1 Tiny\nstep rest 31 0 - rest\nendlayout\n"
		                           "run 1 6 192 1 1 1.1\nlanestate 0 playing 0\nclock 1 128\nend\n",
		                           v16 ),
		       "1/16 view parses" );
		plugin.InjectForTest( v16, {} );
		Frame d = host.Step( plugin, true );
		SavePreview( outDir / "03c_sixteenth_step.png", d );
		const int px1 = PlayheadX( d, 160, H - 60 );
		const float x0px = 32 * u + std::min( 260 * u, W * 0.18f );
		CHECK( std::abs( ( px1 - x0px ) - timeline / 32 ) < 3, "playhead one sixteenth in (%d, expected %.1f)", px1, x0px + timeline / 32 );

		// Not running: no playhead.
		CHECK( seqview::ParseView( "SEQVIEW 1\nstatus ok\nrev abc\nrun 0\nclock 0 0\nend\n", v ), "stop parses" );
		plugin.InjectForTest( v, {} );
		Frame idle = host.Step( plugin, true );
		CHECK( PlayheadX( idle, 160, H - 60 ) < 0, "no playhead when the sequence is not running" );
		SavePreview( outDir / "04_idle.png", idle );

		// Not found: message, no lanes.
		CHECK( seqview::ParseView( "SEQVIEW 1\nstatus not_found\nend\n", v ), "not found parses" );
		plugin.InjectForTest( v, {} );
		Frame nf = host.Step( plugin, true );
		CHECK( Count( nf, 0, 0, W, H, kRed, 10 ) == 0, "no thumbnail when the sequence is not found" );
		SavePreview( outDir / "05_not_found.png", nf );

		// Resize: drawn at the new size at once (no cached viewport).
		plugin.InjectForTest( seqview::View{}, {} );
		FFGLViewportStruct big = { 0, 0, 1920, 1080 };
		plugin.Resize( &big );
		host.Create( 1920, 1080 );
		Frame r = host.Step( plugin, true );
		CHECK( r.lit == static_cast< size_t >( 1920 ) * 1080, "after a resize every pixel of the new size is drawn" );
		host.Create( W, H );
		CHECK( plugin.DeInitGL() == FF_SUCCESS, "DeInitGL" );
	}

	// ---- the real network path against a local test server ----
	{
		printf( "Network...\n" );
		std::vector< uint8_t > redPng = EncodePng( SolidRgba( 64, 36, 255, 0, 0 ).data(), 64, 36 );
		MiniHttp server( [ & ]( const std::string& path ) {
			MiniResponse r;
			// The plugin asks for SEQVIEW 2 (lengths in sixteenths).
			if( path.rfind( "/plugin/v1/view.txt?format=2&name=Loop", 0 ) == 0 )
				r.body = Response( path.find( "&rev=abc" ) == std::string::npos, 48, true );
			else if( path.rfind( "/thumbs/clip/7.png?v=v1", 0 ) == 0 )
				r.contentType = "image/png", r.body.assign( redPng.begin(), redPng.end() );
			else
				r.status = 404;
			return r;
		} );
		CHECK( server.Start(), "test server starts" );

		SeqView plugin;
		FFGLViewportStruct vp = { 0, 0, W, H };
		plugin.InitGL( &vp );
		plugin.SetTextParameter( PT_SERVER, ( "127.0.0.1:" + std::to_string( server.Port() ) ).c_str() );
		plugin.SetTextParameter( PT_SEQUENCE, "Loop" );
		Frame f;
		// Not connected yet: the worker must not poll.
		for( int i = 0; i < 10; ++i )
		{
			host.Step( plugin, false );
			std::this_thread::sleep_for( std::chrono::milliseconds( 10 ) );
		}
		CHECK( server.Requests().empty(), "no polling before Connect (%zu requests)", server.Requests().size() );

		plugin.Connect();
		const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds( 5 );
		while( std::chrono::steady_clock::now() < deadline &&
		       !( plugin.CurrentView().status == seqview::Status::Ok && plugin.TextureCount() == 1 && server.Requests().size() >= 3 ) )
		{
			host.Step( plugin, false );
			std::this_thread::sleep_for( std::chrono::milliseconds( 10 ) );
		}
		f = host.Step( plugin, true );
		CHECK( plugin.CurrentView().status == seqview::Status::Ok, "view fetched over HTTP" );
		CHECK( plugin.TextureCount() == 1, "thumbnail fetched, decoded and uploaded" );
		CHECK( Count( f, 0, 0, W, H / 2 + 60, kRed, 10 ) > 5000, "fetched thumbnail drawn" );
		SavePreview( outDir / "06_network.png", f );
		const auto reqs = server.Requests();
		bool sentRev = false, gotThumb = false;
		for( const auto& p : reqs )
		{
			sentRev |= p.find( "&rev=abc" ) != std::string::npos;
			gotThumb |= p.rfind( "/thumbs/clip/7.png", 0 ) == 0;
		}
		CHECK( sentRev, "later polls send the layout revision back" );
		CHECK( gotThumb, "thumbnail requested" );

		// Disconnect: polling stops.
		plugin.Disconnect();
		std::this_thread::sleep_for( std::chrono::milliseconds( 150 ) );
		const size_t before = server.Requests().size();
		std::this_thread::sleep_for( std::chrono::milliseconds( 300 ) );
		CHECK( server.Requests().size() == before, "no polling after Disconnect (%zu -> %zu)", before, server.Requests().size() );

		// Unreachable server: reported as such.
		plugin.Connect();
		plugin.SetTextParameter( PT_SERVER, "127.0.0.1:1" );
		const auto d2 = std::chrono::steady_clock::now() + std::chrono::seconds( 5 );
		while( std::chrono::steady_clock::now() < d2 && plugin.CurrentView().status != seqview::Status::NoServer )
		{
			host.Step( plugin, false );
			std::this_thread::sleep_for( std::chrono::milliseconds( 20 ) );
		}
		CHECK( plugin.CurrentView().status == seqview::Status::NoServer, "unreachable sequencer reported" );
		SavePreview( outDir / "07_no_server.png", host.Step( plugin, true ) );
		CHECK( plugin.DeInitGL() == FF_SUCCESS, "DeInitGL" );
	}

	// ---- GPU timing sanity at 1080p with a populated view ----
	{
		SeqView plugin;
		FFGLViewportStruct vp = { 0, 0, 1920, 1080 };
		plugin.InitGL( &vp );
		seqview::View v;
		seqview::ParseView( Response( true, 48 ), v );
		seqview::Thumbnail red;
		red.clipId = 7, red.version = "v1", red.width = 320, red.height = 240, red.rgba = SolidRgba( 320, 240, 200, 30, 30 );
		plugin.InjectForTest( v, { red } );
		host.Create( 1920, 1080 );
		GLuint q = 0;
		glGenQueries( 1, &q );
		double totalMs = 0;
		const int frames = 30;
		for( int i = 0; i < frames; ++i )
		{
			glBeginQuery( GL_TIME_ELAPSED, q );
			host.Step( plugin, false );
			glEndQuery( GL_TIME_ELAPSED );
			GLuint64 ns = 0;
			glGetQueryObjectui64v( q, GL_QUERY_RESULT, &ns );
			if( i >= 3 )
				totalMs += ns / 1e6;
		}
		glDeleteQueries( 1, &q );
		const double avgMs = totalMs / ( frames - 3 );
		printf( "  GPU: %.3f ms/frame at 1920x1080\n", avgMs );
		CHECK( avgMs < 2.0, "GPU budget at 1080p (%.3f ms)", avgMs );
		plugin.DeInitGL();
	}

	CHECK( DrainGlErrors() == GL_NO_ERROR, "no GL error at the end" );
	host.Destroy();
	DestroyTestGlContext();

	printf( "\n%d checks, %d failed\n", g_checks, g_failures );
	return g_failures ? 1 : 0;
}
