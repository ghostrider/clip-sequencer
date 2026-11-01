#include "SeqView.h"
#include "BuildInfo.h" //generated at build time into the intermediate directory

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <string>

//Build number and date, generated into BuildInfo.h by the SeqView.vcxproj GenerateBuildInfo target on every build.
#define SQV_STR2( x ) #x
#define SQV_STR( x ) SQV_STR2( x )
static const char kBuildInfo[] = "Build " SQV_STR( SEQVIEW_BUILD_NUMBER ) " - " SEQVIEW_BUILD_DATE;

static_assert( PT_COUNT == 3, "update the constructor when adding parameters" );

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
	"Clip Sequencer SeqView, " SQV_STR( SEQVIEW_BUILD_NUMBER ) " (" SEQVIEW_BUILD_DATE ")" // About
);

static std::atomic< int > g_instances{ 0 };

static void Log( int instance, const std::string& text )
{
	FFGLLog::LogToHost( ( "SeqView#" + std::to_string( instance ) + " " + text ).c_str() );
}

static const char* StatusName( seqview::Status s )
{
	switch( s )
	{
	case seqview::Status::NoName: return "no sequence assigned";
	case seqview::Status::NoServer: return "sequencer not reachable";
	case seqview::Status::NotFound: return "sequence not found";
	case seqview::Status::BadResponse: return "unexpected response";
	case seqview::Status::Ok: return "ok";
	}
	return "?";
}

SeqView::SeqView() : instance( ++g_instances )
{
	SetMinInputs( 0 );
	SetMaxInputs( 0 );
	SetParamInfo( PT_SEQUENCE, "Sequence", FF_TYPE_TEXT, "" );
	SetParamInfo( PT_SERVER, "Server", FF_TYPE_TEXT, kDefaultServer );
	SetParamInfo( PT_BUILD, "Build", FF_TYPE_TEXT, kBuildInfo );
}

SeqView::~SeqView()
{
	client.Stop(); // joins the worker; no GL here
}

FFResult SeqView::InitGL( const FFGLViewportStruct* vp )
{
	Log( instance, std::string( "InitGL (" ) + kBuildInfo + ")" );
	if( !renderer.Init() )
		Log( instance, "renderer init failed (shader or quad) - nothing will be drawn" );
	if( !renderer.FontReady() )
		Log( instance, "no font (segoeui.ttf / arial.ttf) - text will be missing" );
	client.SetTarget( Text( PT_SERVER ), Text( PT_SEQUENCE ) );
	client.Start();
	return CFFGLPlugin::InitGL( vp );
}

FFResult SeqView::DeInitGL()
{
	client.Stop();
	renderer.Release();
	return FF_SUCCESS;
}

unsigned int SeqView::Connect()
{
	client.SetActive( true );
	return FF_SUCCESS;
}

unsigned int SeqView::Disconnect()
{
	client.SetActive( false );
	return FF_SUCCESS;
}

FFResult SeqView::ProcessOpenGL( ProcessOpenGLStruct* )
{
	seqview::Update update = client.Take();
	if( update.viewChanged )
	{
		view       = std::move( update.view );
		receivedAt = update.receivedAt;
		if( view.status != loggedStatus )
		{
			Log( instance, std::string( "'" ) + Text( PT_SEQUENCE ) + "': " + StatusName( view.status ) );
			loggedStatus = view.status;
		}
	}
	renderer.Upload( update.thumbnails );

	// Playhead between two polls: advance at the reported tempo (at most a quarter second ahead).
	const seqview::Runtime& r = view.runtime;
	double pos = static_cast< double >( r.pos );
	if( r.running && r.clockRunning && r.bpm > 0 )
	{
		const double elapsed = std::min( std::chrono::duration< double >( std::chrono::steady_clock::now() - receivedAt ).count(), 0.25 );
		pos += elapsed * r.bpm * 24.0 / 60.0;
	}
	if( r.passTicks > 0 )
		pos = std::fmod( pos, static_cast< double >( r.passTicks ) );

	// Viewport read fresh every frame (CFFGLPlugin only updates currentViewport in InitGL/Resize).
	const FFGLViewportStruct vp = currentViewport;
	glViewport( vp.x, vp.y, vp.width, vp.height );
	renderer.Draw( view, pos, static_cast< int >( vp.width ), static_cast< int >( vp.height ), Text( PT_SEQUENCE ), Text( PT_SERVER ) );
	return FF_SUCCESS;
}

FFResult SeqView::SetTextParameter( unsigned int dwIndex, const char* value )
{
	if( dwIndex == PT_SEQUENCE || dwIndex == PT_SERVER )
	{
		std::string v = value ? value : "";
		if( dwIndex == PT_SERVER && v.empty() )
			v = kDefaultServer;
		std::string seq, srv;
		{
			std::lock_guard< std::mutex > lock( textMutex );
			( dwIndex == PT_SEQUENCE ? sequence : server ) = v;
			seq = sequence;
			srv = server;
		}
		client.SetTarget( srv, seq );
		return FF_SUCCESS;
	}
	//The build info is display-only: edits from the host are accepted but ignored (repo convention).
	return dwIndex == PT_BUILD ? FF_SUCCESS : FF_FAIL;
}

char* SeqView::GetTextParameter( unsigned int index )
{
	if( index == PT_SEQUENCE || index == PT_SERVER )
	{
		textBuffer = Text( index );
		return const_cast< char* >( textBuffer.c_str() );
	}
	if( index == PT_BUILD )
		return const_cast< char* >( kBuildInfo );
	return (char*)FF_FAIL;
}

std::string SeqView::Text( unsigned int index ) const
{
	std::lock_guard< std::mutex > lock( textMutex );
	return index == PT_SEQUENCE ? sequence : server;
}
