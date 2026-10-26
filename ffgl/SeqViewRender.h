#pragma once
// Draws a sequence view: name and state on top, one row per lane with steps as blocks (clip thumbnails, rests,
// unresolved steps), bar lines and the playhead. Colours are the sequencer UI's design tokens.
//
// All methods are GL calls and must run on the render thread with the host's context current (InitGL /
// ProcessOpenGL / DeInitGL). Coordinates are framebuffer pixels with the origin at the top left.
#include <cstdint>
#include <map>
#include <string>
#include <vector>

#include <FFGLSDK.h>

#include "SeqViewClient.h"
#include "SeqViewModel.h"

// Declarations only; SeqViewRender.cpp includes it a second time with STB_TRUETYPE_IMPLEMENTATION (Text3D pattern).
#pragma warning( push )
#pragma warning( disable : 4244 4267 4456 4457 4996 )
#include "stb_truetype.h"
#pragma warning( pop )

namespace seqview
{
struct Rgba
{
	float r, g, b, a;
};

class Renderer
{
public:
	Renderer();
	~Renderer();

	bool Init();    // shader, quad, font atlas
	void Release(); // frees every GL object
	void Upload( std::vector< Thumbnail >& thumbnails );

	//! `posTicks`: playhead position within the pass (already interpolated by the caller).
	void Draw( const View& view, double posTicks, int width, int height, const std::string& sequence, const std::string& server );

	bool FontReady() const { return fontTexture != 0; }
	size_t TextureCount() const { return textures.size(); }

	//! Width in pixels of `text` at `px` pixel height (no GL).
	float TextWidth( const std::string& text, float px ) const;

private:
	struct Texture
	{
		GLuint id = 0;
		int width = 0, height = 0;
		std::string version;
	};

	void Fill( float x0, float y0, float x1, float y1, Rgba c );
	void Frame( float x0, float y0, float x1, float y1, float t, Rgba c );
	void Image( float x0, float y0, float x1, float y1, const Texture& tex, float alpha );
	//! Draws text with its baseline at `y`; truncates with "..." to `maxWidth`. Returns the drawn width.
	float Text( float x, float y, float px, const std::string& text, Rgba c, float maxWidth = 1e9f );
	void Centered( float cx, float cy, float px, const std::string& text, Rgba c );

	ffglex::FFGLShader shader;
	ffglex::FFGLScreenQuad quad;
	GLuint fontTexture = 0;
	std::vector< stbtt_bakedchar > glyphs;
	std::map< uint64_t, Texture > textures;
	int vw = 1, vh = 1;
};

//! UTF-8 -> code points (invalid bytes become '?').
std::vector< uint32_t > DecodeUtf8( const std::string& s );
} // namespace seqview
