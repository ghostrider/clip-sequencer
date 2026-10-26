#include "SeqView.h"
#include "BuildInfo.h" //generated at build time into the intermediate directory

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <string>

//Build number and date, generated into BuildInfo.h by the SeqView.vcxproj GenerateBuildInfo target on every build.
#define SQV_STR2( x ) #x
#define SQV_STR( x ) SQV_STR2( x )
static const char kBuildInfo[] = "Build " SQV_STR( SEQVIEW_BUILD_NUMBER ) " - " SEQVIEW_BUILD_DATE;

static_assert( PT_COUNT == 2, "update the constructor when adding parameters" );

static CFFGLPluginInfo PluginInfo(
	PluginFactory< SeqView >,                                                // Create method
	"VJSQ",                                                                  // Plugin unique ID of maximum length 4.
	"1000A Sequence",                                                        // Plugin name (max 16 chars)
	2,                                                                       // API major version number
	1,                                                                       // API minor version number
	1,                                                                       // Plugin major version number
	0,                                                                       // Plugin minor version number
	FF_SOURCE,                                                               // Plugin type
	"Shows a sequence of the Resolume Clip Sequencer (placeholder for § clips)", // Description
	"vj_stuff SeqView, " SQV_STR( SEQVIEW_BUILD_NUMBER ) " (" SEQVIEW_BUILD_DATE ")" // About
);

static std::atomic< int > g_instances{ 0 };

static void Log( int instance, const std::string& text )
{
	FFGLLog::LogToHost( ( "SeqView#" + std::to_string( instance ) + " " + text ).c_str() );
}

//! A stable, clearly visible colour per sequence name (FNV-1a hash -> hue), so a parameter change shows on screen.
static void ColourFor( const std::string& name, float rgb[ 3 ] )
{
	uint32_t h = 2166136261u;
	for( unsigned char c : name )
		h = ( h ^ c ) * 16777619u;
	const float hue = name.empty() ? 0.0f : ( h % 360u ) / 60.0f;
	const float x   = 1.0f - std::abs( std::fmod( hue, 2.0f ) - 1.0f );
	const float c   = name.empty() ? 0.25f : 0.8f; // empty name: dark grey
	float r = 0, g = 0, b = 0;
	if( name.empty() )
		r = g = b = 1.0f;
	else if( hue < 1 )
		r = 1, g = x;
	else if( hue < 2 )
		r = x, g = 1;
	else if( hue < 3 )
		g = 1, b = x;
	else if( hue < 4 )
		g = x, b = 1;
	else if( hue < 5 )
		r = x, b = 1;
	else
		r = 1, b = x;
	rgb[ 0 ] = r * c;
	rgb[ 1 ] = g * c;
	rgb[ 2 ] = b * c;
}

SeqView::SeqView() : instance( ++g_instances )
{
	SetMinInputs( 0 );
	SetMaxInputs( 0 );
	SetParamInfo( PT_SEQUENCE, "Sequence", FF_TYPE_TEXT, "" );
	SetParamInfo( PT_BUILD, "Build", FF_TYPE_TEXT, kBuildInfo );
	Log( instance, std::string( "created (" ) + kBuildInfo + ")" );
}

SeqView::~SeqView()
{
	Log( instance, "destroyed" );
}

FFResult SeqView::InitGL( const FFGLViewportStruct* vp )
{
	Log( instance, "InitGL " + std::to_string( vp->width ) + "x" + std::to_string( vp->height ) );
	statsStart     = std::chrono::steady_clock::now();
	framesSinceLog = 0;
	return CFFGLPlugin::InitGL( vp );
}

FFResult SeqView::DeInitGL()
{
	Log( instance, "DeInitGL" );
	return FF_SUCCESS;
}

unsigned int SeqView::Resize( const FFGLViewportStruct* vp )
{
	Log( instance, "Resize " + std::to_string( vp->width ) + "x" + std::to_string( vp->height ) );
	return CFFGLPlugin::Resize( vp );
}

unsigned int SeqView::Connect()
{
	Log( instance, "Connect" );
	return FF_SUCCESS;
}

unsigned int SeqView::Disconnect()
{
	Log( instance, "Disconnect" );
	return FF_SUCCESS;
}

FFResult SeqView::ProcessOpenGL( ProcessOpenGLStruct* )
{
	// Viewport read fresh every frame (CFFGLPlugin only updates currentViewport in InitGL/Resize).
	const FFGLViewportStruct vp = currentViewport;
	float rgb[ 3 ];
	ColourFor( Sequence(), rgb );
	glViewport( vp.x, vp.y, vp.width, vp.height );
	glClearColor( rgb[ 0 ], rgb[ 1 ], rgb[ 2 ], 1.0f );
	glClear( GL_COLOR_BUFFER_BIT );

	++framesSinceLog;
	const auto now = std::chrono::steady_clock::now();
	if( now - statsStart >= std::chrono::seconds( 1 ) )
	{
		const double secs = std::chrono::duration< double >( now - statsStart ).count();
		char line[ 256 ];
		snprintf( line, sizeof( line ), "rendering: %u frames in %.1f s at %ux%u, sequence '%s'", framesSinceLog, secs,
		          vp.width, vp.height, Sequence().c_str() );
		Log( instance, line );
		statsStart     = now;
		framesSinceLog = 0;
	}
	return FF_SUCCESS;
}

FFResult SeqView::SetTextParameter( unsigned int dwIndex, const char* value )
{
	if( dwIndex == PT_SEQUENCE )
	{
		const std::string v = value ? value : "";
		{
			std::lock_guard< std::mutex > lock( sequenceMutex );
			if( v == sequence )
				return FF_SUCCESS;
			sequence = v;
		}
		Log( instance, "Sequence = '" + v + "'" );
		return FF_SUCCESS;
	}
	//The build info is display-only: edits from the host are accepted but ignored (repo convention).
	return dwIndex == PT_BUILD ? FF_SUCCESS : FF_FAIL;
}

char* SeqView::GetTextParameter( unsigned int index )
{
	if( index == PT_SEQUENCE )
	{
		textBuffer = Sequence();
		return const_cast< char* >( textBuffer.c_str() );
	}
	if( index == PT_BUILD )
		return const_cast< char* >( kBuildInfo );
	return (char*)FF_FAIL;
}

std::string SeqView::Sequence() const
{
	std::lock_guard< std::mutex > lock( sequenceMutex );
	return sequence;
}
