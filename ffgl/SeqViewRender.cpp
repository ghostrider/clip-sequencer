#include "SeqViewRender.h"

//stb_truetype's implementation, generated exactly once (here).
#define STB_TRUETYPE_IMPLEMENTATION
#pragma warning( push )
#pragma warning( disable : 4244 4267 4456 4457 4996 )
#include "stb_truetype.h"
#pragma warning( pop )

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <iterator>

using namespace ffglex;

namespace seqview
{
namespace
{
// Design tokens of the sequencer UI (docs/ui/editor-mockup.html :root).
constexpr Rgba kBg{ 0x0E / 255.f, 0x0F / 255.f, 0x11 / 255.f, 1 };
constexpr Rgba kTrack{ 0x12 / 255.f, 0x13 / 255.f, 0x16 / 255.f, 1 };
constexpr Rgba kControl{ 0x1C / 255.f, 0x1E / 255.f, 0x22 / 255.f, 1 };
constexpr Rgba kLine{ 0x26 / 255.f, 0x29 / 255.f, 0x2E / 255.f, 1 };
constexpr Rgba kLineStrong{ 0x33 / 255.f, 0x37 / 255.f, 0x3D / 255.f, 1 };
constexpr Rgba kText{ 0xE6 / 255.f, 0xE6 / 255.f, 0xE3 / 255.f, 1 };
constexpr Rgba kText2{ 0xC9 / 255.f, 0xCC / 255.f, 0xD1 / 255.f, 1 };
constexpr Rgba kMuted{ 0x9A / 255.f, 0x9E / 255.f, 0xA6 / 255.f, 1 };
constexpr Rgba kAccent{ 0xF2 / 255.f, 0xA3 / 255.f, 0x3A / 255.f, 1 };
constexpr Rgba kAccentText{ 0xFF / 255.f, 0xD3 / 255.f, 0x9A / 255.f, 1 };
constexpr Rgba kErr{ 0xE5 / 255.f, 0x48 / 255.f, 0x4D / 255.f, 1 };
constexpr Rgba kErrBg{ 0x1E / 255.f, 0x12 / 255.f, 0x14 / 255.f, 1 };
constexpr Rgba kErrText{ 0xFF / 255.f, 0xB4 / 255.f, 0xB7 / 255.f, 1 };

constexpr float kBakePx    = 48.0f; // glyph atlas resolution; drawn scaled
constexpr int kAtlasSize   = 1024;
constexpr int kFirstChar   = 32;
constexpr int kCharCount   = 224; // 32..255 (Latin-1: covers § and umlauts)
constexpr float kAscent    = 0.75f; // of the pixel height, for centring text

const char kVertex[] =
	"#version 410 core\n"
	"layout( location = 0 ) in vec4 aPosition;\n"
	"layout( location = 1 ) in vec2 aUV;\n"
	"uniform vec4 uRect;     // x0 y0 x1 y1, pixels, origin top left\n"
	"uniform vec4 uTex;      // s0 t0 s1 t1, t0 = top row\n"
	"uniform vec2 uViewport;\n"
	"out vec2 vTex;\n"
	"void main()\n"
	"{\n"
	"    vec2 corner = vec2( aUV.x, 1.0 - aUV.y ); // the quad's uv has v = 1 at the top\n"
	"    vec2 p = mix( uRect.xy, uRect.zw, corner );\n"
	"    vTex = mix( uTex.xy, uTex.zw, corner );\n"
	"    gl_Position = vec4( p.x / uViewport.x * 2.0 - 1.0, 1.0 - p.y / uViewport.y * 2.0, 0.0, 1.0 );\n"
	"}\n";

const char kFragment[] =
	"#version 410 core\n"
	"in vec2 vTex;\n"
	"uniform int uMode;           // 0 solid, 1 image, 2 glyph (atlas red channel = coverage)\n"
	"uniform vec4 uColor;\n"
	"uniform sampler2D uTexture;\n"
	"out vec4 fragColor;\n"
	"void main()\n"
	"{\n"
	"    if( uMode == 0 )\n"
	"        fragColor = uColor;\n"
	"    else if( uMode == 1 )\n"
	"    {\n"
	"        vec4 t = texture( uTexture, vTex );\n"
	"        fragColor = vec4( t.rgb * uColor.rgb, t.a * uColor.a );\n"
	"    }\n"
	"    else\n"
	"        fragColor = vec4( uColor.rgb, texture( uTexture, vTex ).r * uColor.a );\n"
	"}\n";

std::vector< unsigned char > ReadFile( const std::string& path )
{
	std::ifstream f( path, std::ios::binary );
	return std::vector< unsigned char >( std::istreambuf_iterator< char >( f ), {} );
}

std::string Format( const char* fmt, double a, double b = 0 )
{
	char buf[ 64 ];
	snprintf( buf, sizeof( buf ), fmt, a, b );
	return buf;
}
} // namespace

std::vector< uint32_t > DecodeUtf8( const std::string& s )
{
	std::vector< uint32_t > out;
	for( size_t i = 0; i < s.size(); )
	{
		const unsigned char c = static_cast< unsigned char >( s[ i ] );
		uint32_t cp = '?';
		size_t n    = 1;
		if( c < 0x80 )
			cp = c;
		else if( ( c >> 5 ) == 6 && i + 1 < s.size() )
			cp = ( ( c & 0x1F ) << 6 ) | ( s[ i + 1 ] & 0x3F ), n = 2;
		else if( ( c >> 4 ) == 14 && i + 2 < s.size() )
			cp = ( ( c & 0x0F ) << 12 ) | ( ( s[ i + 1 ] & 0x3F ) << 6 ) | ( s[ i + 2 ] & 0x3F ), n = 3;
		else if( ( c >> 3 ) == 30 && i + 3 < s.size() )
			cp = '?', n = 4;
		out.push_back( cp );
		i += n;
	}
	return out;
}

Renderer::Renderer() = default;
Renderer::~Renderer() = default; // GL objects are freed in Release() (render thread), never here

bool Renderer::Init()
{
	bool ok = shader.Compile( kVertex, kFragment );
	ok      = quad.Initialise() && ok;

	// Font: Segoe UI from the Windows fonts folder (always present on Windows 10/11); Arial as fallback.
	std::string windows = "C:\\Windows";
	char* windir        = nullptr;
	size_t windirLen    = 0;
	if( _dupenv_s( &windir, &windirLen, "WINDIR" ) == 0 && windir )
	{
		windows = windir;
		free( windir );
	}
	const std::string fonts = windows + "\\Fonts\\";
	std::vector< unsigned char > ttf = ReadFile( fonts + "segoeui.ttf" );
	if( ttf.empty() )
		ttf = ReadFile( fonts + "arial.ttf" );
	if( !ttf.empty() )
	{
		std::vector< unsigned char > atlas( static_cast< size_t >( kAtlasSize ) * kAtlasSize );
		glyphs.resize( kCharCount );
		if( stbtt_BakeFontBitmap( ttf.data(), 0, kBakePx, atlas.data(), kAtlasSize, kAtlasSize, kFirstChar, kCharCount, glyphs.data() ) > 0 )
		{
			glGenTextures( 1, &fontTexture );
			Scoped2DTextureBinding bind( fontTexture );
			glPixelStorei( GL_UNPACK_ALIGNMENT, 1 );
			glTexImage2D( GL_TEXTURE_2D, 0, GL_R8, kAtlasSize, kAtlasSize, 0, GL_RED, GL_UNSIGNED_BYTE, atlas.data() );
			glPixelStorei( GL_UNPACK_ALIGNMENT, 4 );
			glTexParameteri( GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR );
			glTexParameteri( GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR );
			glTexParameteri( GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE );
			glTexParameteri( GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE );
		}
		else
			glyphs.clear();
	}
	return ok;
}

void Renderer::Release()
{
	shader.FreeGLResources();
	quad.Release();
	if( fontTexture )
		glDeleteTextures( 1, &fontTexture );
	fontTexture = 0;
	for( auto& [ id, t ] : textures )
		glDeleteTextures( 1, &t.id );
	textures.clear();
}

void Renderer::Upload( std::vector< Thumbnail >& thumbnails )
{
	for( auto& th : thumbnails )
	{
		if( th.rgba.empty() || th.width <= 0 || th.height <= 0 )
			continue;
		Texture& t = textures[ th.clipId ];
		if( !t.id )
			glGenTextures( 1, &t.id );
		Scoped2DTextureBinding bind( t.id );
		glTexImage2D( GL_TEXTURE_2D, 0, GL_RGBA8, th.width, th.height, 0, GL_RGBA, GL_UNSIGNED_BYTE, th.rgba.data() );
		glTexParameteri( GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR );
		glTexParameteri( GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR );
		glTexParameteri( GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE );
		glTexParameteri( GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE );
		t.width   = th.width;
		t.height  = th.height;
		t.version = th.version;
	}
	thumbnails.clear();
}

void Renderer::Fill( float x0, float y0, float x1, float y1, Rgba c )
{
	if( x1 <= x0 || y1 <= y0 )
		return;
	shader.Set( "uMode", 0 );
	shader.Set( "uRect", x0, y0, x1, y1 );
	shader.Set( "uColor", c.r, c.g, c.b, c.a );
	quad.Draw();
}

void Renderer::Frame( float x0, float y0, float x1, float y1, float t, Rgba c )
{
	Fill( x0, y0, x1, y0 + t, c );
	Fill( x0, y1 - t, x1, y1, c );
	Fill( x0, y0 + t, x0 + t, y1 - t, c );
	Fill( x1 - t, y0 + t, x1, y1 - t, c );
}

void Renderer::Image( float x0, float y0, float x1, float y1, const Texture& tex, float alpha )
{
	if( x1 <= x0 || y1 <= y0 || !tex.id )
		return;
	// Cover: crop the image to the block's aspect ratio, centred.
	const float boxAspect = ( x1 - x0 ) / ( y1 - y0 );
	const float imgAspect = static_cast< float >( tex.width ) / tex.height;
	float s0 = 0, t0 = 0, s1 = 1, t1 = 1;
	if( imgAspect > boxAspect )
	{
		const float w = boxAspect / imgAspect;
		s0            = 0.5f - w / 2;
		s1            = 0.5f + w / 2;
	}
	else
	{
		const float h = imgAspect / boxAspect;
		t0            = 0.5f - h / 2;
		t1            = 0.5f + h / 2;
	}
	ScopedSamplerActivation sampler( 0 );
	Scoped2DTextureBinding bind( tex.id );
	shader.Set( "uMode", 1 );
	shader.Set( "uRect", x0, y0, x1, y1 );
	shader.Set( "uTex", s0, t0, s1, t1 );
	shader.Set( "uColor", 1.0f, 1.0f, 1.0f, alpha );
	quad.Draw();
}

float Renderer::TextWidth( const std::string& text, float px ) const
{
	if( glyphs.empty() )
		return 0;
	const float scale = px / kBakePx;
	float w           = 0;
	for( uint32_t cp : DecodeUtf8( text ) )
	{
		const int i = ( cp >= kFirstChar && cp < kFirstChar + kCharCount ) ? static_cast< int >( cp ) - kFirstChar : '?' - kFirstChar;
		w += glyphs[ i ].xadvance * scale;
	}
	return w;
}

float Renderer::Text( float x, float y, float px, const std::string& text, Rgba c, float maxWidth )
{
	if( !fontTexture || glyphs.empty() || px < 2 )
		return 0;
	std::string s = text;
	if( TextWidth( s, px ) > maxWidth )
	{
		const float dots = TextWidth( "...", px );
		std::vector< uint32_t > cps = DecodeUtf8( s );
		const float scale = px / kBakePx;
		float w           = 0;
		size_t keep       = 0;
		for( ; keep < cps.size(); ++keep )
		{
			const uint32_t cp = cps[ keep ];
			const int i = ( cp >= kFirstChar && cp < kFirstChar + kCharCount ) ? static_cast< int >( cp ) - kFirstChar : '?' - kFirstChar;
			if( w + glyphs[ i ].xadvance * scale + dots > maxWidth )
				break;
			w += glyphs[ i ].xadvance * scale;
		}
		s.clear();
		for( size_t k = 0; k < keep; ++k ) // re-encode the kept code points (all < 256) as UTF-8
		{
			const uint32_t cp = cps[ k ];
			if( cp < 0x80 )
				s += static_cast< char >( cp );
			else
			{
				s += static_cast< char >( 0xC0 | ( cp >> 6 ) );
				s += static_cast< char >( 0x80 | ( cp & 0x3F ) );
			}
		}
		if( maxWidth >= dots )
			s += "...";
	}
	ScopedSamplerActivation sampler( 0 );
	Scoped2DTextureBinding bind( fontTexture );
	shader.Set( "uMode", 2 );
	shader.Set( "uColor", c.r, c.g, c.b, c.a );
	const float scale = px / kBakePx;
	float bx = 0, by = 0; // baked units, y down from the baseline
	for( uint32_t cp : DecodeUtf8( s ) )
	{
		const int i = ( cp >= kFirstChar && cp < kFirstChar + kCharCount ) ? static_cast< int >( cp ) - kFirstChar : '?' - kFirstChar;
		stbtt_aligned_quad q;
		stbtt_GetBakedQuad( glyphs.data(), kAtlasSize, kAtlasSize, i, &bx, &by, &q, 1 );
		if( q.x1 > q.x0 )
		{
			shader.Set( "uRect", x + q.x0 * scale, y + q.y0 * scale, x + q.x1 * scale, y + q.y1 * scale );
			shader.Set( "uTex", q.s0, q.t0, q.s1, q.t1 );
			quad.Draw();
		}
	}
	return bx * scale;
}

void Renderer::Centered( float cx, float cy, float px, const std::string& text, Rgba c )
{
	Text( cx - TextWidth( text, px ) / 2, cy + px * kAscent / 2, px, text, c );
}

void Renderer::Draw( const View& view, double posTicks, int width, int height, const std::string& sequence, const std::string& server )
{
	vw = std::max( width, 1 );
	vh = std::max( height, 1 );
	if( !shader.IsReady() )
		return;
	const float W = static_cast< float >( vw ), H = static_cast< float >( vh );
	const float u = H / 1080.0f; // design unit: 1 px at 1080p

	ScopedShaderBinding bindShader( shader.GetGLID() );
	shader.Set( "uViewport", W, H );
	shader.Set( "uTexture", 0 );
	shader.Set( "uTex", 0.0f, 0.0f, 1.0f, 1.0f );

	// Opaque background first, without blending (alpha 1 everywhere), then blend the rest over it.
	Fill( 0, 0, W, H, kBg );
	const GLboolean blendWasEnabled = glIsEnabled( GL_BLEND );
	glEnable( GL_BLEND );
	glBlendFuncSeparate( GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA, GL_ONE, GL_ONE_MINUS_SRC_ALPHA );

	const float margin = 32 * u;
	const float title  = 72 * u;

	auto message = [ & ]( const std::string& headline, const std::string& detail, Rgba c ) {
		Centered( W / 2, H / 2 - 40 * u, 64 * u, headline, c );
		Centered( W / 2, H / 2 + 50 * u, 36 * u, detail, kMuted );
	};

	if( view.status != Status::Ok || !view.hasLayout )
	{
		const std::string label = "\xC2\xA7 " + ( sequence.empty() ? std::string( "Sequence" ) : sequence ); // "§ name"
		switch( view.status )
		{
		case Status::NoName:
			message( "\xC2\xA7 Sequence", "No sequence assigned - name this clip \"\xC2\xA7 <sequence>\"", kText2 );
			break;
		case Status::NoServer:
			message( label, "Sequencer not reachable at " + server, kErrText );
			break;
		case Status::NotFound:
			message( label, "No sequence named \"" + sequence + "\"", kErrText );
			break;
		case Status::BadResponse:
			message( label, "Unexpected answer from " + server, kErrText );
			break;
		case Status::Ok:
			message( label, "Loading...", kMuted );
			break;
		}
		if( !blendWasEnabled )
			glDisable( GL_BLEND );
		return;
	}

	const Layout& L  = view.layout;
	const Runtime& R = view.runtime;

	// ---- header: name, deck/length, state ----
	const float nameW = Text( margin, margin + title * 0.8f, title, L.name, kText, W * 0.6f );
	const float bars  = L.passEighths / 8.0f;
	const std::string sub = L.deck + "  \xC2\xB7  " + ( std::fmod( bars, 1.0f ) == 0 ? Format( "%.0f bars", bars ) : Format( "%.1f bars", bars ) );
	Text( margin + std::min( nameW, W * 0.6f ) + 24 * u, margin + title * 0.8f, 32 * u, sub, kMuted, W * 0.25f );
	std::string state = "idle";
	Rgba stateColor   = kMuted;
	if( R.running )
	{
		state = R.barBeat + "   pass " + std::to_string( R.pass ) + ( R.passes ? " / " + std::to_string( R.passes ) : std::string( "" ) );
		stateColor = R.clockRunning ? kAccentText : kErrText;
		if( !R.clockRunning )
			state += "   no clock";
	}
	const float stateW = TextWidth( state, 44 * u );
	Text( W - margin - stateW, margin + title * 0.8f, 44 * u, state, stateColor );

	// ---- lanes ----
	const float top     = margin + title + 24 * u;
	const float bottom  = H - margin;
	const size_t n      = std::max< size_t >( L.lanes.size(), 1 );
	const float rowGap  = 8 * u;
	const float rowH    = ( bottom - top - rowGap * ( n - 1 ) ) / n; // lanes share the full height
	const float headW   = std::min( 260 * u, W * 0.18f );
	const float x0      = margin + headW;
	const float x1      = W - margin;
	const float pass    = static_cast< float >( std::max< uint32_t >( L.passEighths, 1 ) );
	const float perEighth = ( x1 - x0 ) / pass;
	const float lanesBottom = top + n * rowH + ( n - 1 ) * rowGap;

	Fill( x0, top, x1, lanesBottom, kTrack );
	for( float e = 0; e <= pass + 0.001f; e += 8 )
		Fill( x0 + e * perEighth - 1 * u, top, x0 + e * perEighth + 1 * u, lanesBottom, kLineStrong );

	for( size_t li = 0; li < L.lanes.size(); ++li )
	{
		const Lane& lane = L.lanes[ li ];
		const float y0   = top + li * ( rowH + rowGap );
		const float y1   = y0 + rowH;
		const bool laneOk = lane.state == "ok";
		const LaneState* ls = li < R.lanes.size() ? &R.lanes[ li ] : nullptr;
		Fill( margin, y0, x0 - 8 * u, y1, kControl );
		const float headPx = std::min( 40 * u, rowH * 0.32f );
		Text( margin + 14 * u, y0 + rowH * 0.42f + headPx * 0.4f, headPx, lane.layer, laneOk ? kText : kErrText, headW - 36 * u );
		std::string sub2 = laneOk ? "" : ( lane.state == "missing" ? "layer missing" : lane.state == "ambiguous" ? "layer ambiguous" : "SEQ layer" );
		if( ls && R.running && laneOk )
			sub2 = ls->state == "resume_pending" ? "resuming" : ls->state == "taken_over" ? "taken over" : ls->state;
		Text( margin + 14 * u, y0 + rowH * 0.42f + headPx * 1.3f, headPx * 0.6f, sub2,
		      ls && ls->state == "yielded" ? kErrText : ls && ls->state == "playing" ? kAccentText : kMuted, headW - 36 * u );

		float e = 0;
		for( size_t si = 0; si < lane.steps.size() && e < pass; ++si )
		{
			const Step& st = lane.steps[ si ];
			const float bx0 = x0 + e * perEighth + 2 * u;
			e += static_cast< float >( st.eighths );
			const float bx1 = x0 + std::min( e, pass ) * perEighth - 2 * u;
			const float by0 = y0 + 6 * u, by1 = y1 - 6 * u;
			const bool playing = R.running && ls && ls->state == "playing" && ls->step == static_cast< int >( si );
			if( st.kind == StepKind::Rest )
			{
				Frame( bx0, by0, bx1, by1, 2 * u, kLineStrong );
				Text( bx0 + 10 * u, by1 - 12 * u, std::min( 28 * u, rowH * 0.22f ), "rest", kMuted, bx1 - bx0 - 20 * u );
				continue;
			}
			if( st.kind == StepKind::Unresolved )
			{
				Fill( bx0, by0, bx1, by1, kErrBg );
				Frame( bx0, by0, bx1, by1, 2 * u, kErr );
			}
			else
			{
				Fill( bx0, by0, bx1, by1, kControl );
				auto tex = textures.find( st.clipId );
				if( tex != textures.end() )
					Image( bx0, by0, bx1, by1, tex->second, 1.0f );
				// darken the lower part so the label stays readable
				Fill( bx0, by1 - rowH * 0.32f, bx1, by1, Rgba{ kBg.r, kBg.g, kBg.b, 0.65f } );
				Frame( bx0, by0, bx1, by1, 1 * u, kLine );
			}
			Text( bx0 + 10 * u, by1 - 12 * u, std::min( 28 * u, rowH * 0.22f ), st.label, st.kind == StepKind::Unresolved ? kErrText : kText,
			      bx1 - bx0 - 20 * u );
			if( playing )
				Fill( bx0, by0, bx1, by0 + 6 * u, kAccent );
		}
	}

	// ---- playhead ----
	if( R.running )
	{
		const float x = x0 + static_cast< float >( posTicks / 12.0 ) * perEighth;
		Fill( x - 2 * u, top - 10 * u, x + 2 * u, lanesBottom, kAccent );
	}

	if( !blendWasEnabled )
		glDisable( GL_BLEND );
}
} // namespace seqview
