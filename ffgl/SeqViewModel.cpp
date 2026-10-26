#include "SeqViewModel.h"

#include <cstdlib>
#include <sstream>

namespace seqview
{
namespace
{
//! Splits off `n` space-separated fields; the rest of the line (may contain spaces) goes to `rest`.
bool Fields( const std::string& line, size_t n, std::vector< std::string >& out, std::string* rest = nullptr )
{
	out.clear();
	size_t pos = 0;
	for( size_t i = 0; i < n; ++i )
	{
		const size_t sp = line.find( ' ', pos );
		if( sp == std::string::npos )
		{
			if( i + 1 == n && !rest )
			{
				out.push_back( line.substr( pos ) );
				return true;
			}
			return false;
		}
		out.push_back( line.substr( pos, sp - pos ) );
		pos = sp + 1;
	}
	if( rest )
		*rest = pos <= line.size() ? line.substr( pos ) : std::string();
	return true;
}

uint64_t U64( const std::string& s ) { return std::strtoull( s.c_str(), nullptr, 10 ); }
} // namespace

bool ParseView( const std::string& text, View& view )
{
	std::istringstream in( text );
	std::string line;
	if( !std::getline( in, line ) || line != "SEQVIEW 1" )
	{
		view.status = Status::BadResponse;
		return false;
	}
	Runtime runtime;
	Layout layout;
	bool inLayout = false, gotLayout = false, gotEnd = false;
	std::vector< std::string > f;
	std::string rest;
	while( std::getline( in, line ) )
	{
		if( !line.empty() && line.back() == '\r' )
			line.pop_back();
		if( inLayout )
		{
			if( line == "endlayout" )
			{
				inLayout  = false;
				gotLayout = true;
			}
			else if( line.rfind( "name ", 0 ) == 0 )
				layout.name = line.substr( 5 );
			else if( line.rfind( "deck ", 0 ) == 0 )
				layout.deck = line.substr( 5 );
			else if( line.rfind( "pass ", 0 ) == 0 )
				layout.passEighths = static_cast< uint32_t >( U64( line.substr( 5 ) ) );
			else if( line.rfind( "lane ", 0 ) == 0 && Fields( line, 3, f, &rest ) )
				layout.lanes.push_back( Lane{ f[ 1 ], f[ 2 ], rest, {} } );
			else if( line.rfind( "step ", 0 ) == 0 && Fields( line, 5, f, &rest ) && !layout.lanes.empty() )
			{
				Step s;
				s.kind    = f[ 1 ] == "clip" ? StepKind::Clip : f[ 1 ] == "rest" ? StepKind::Rest : StepKind::Unresolved;
				s.eighths = static_cast< uint32_t >( U64( f[ 2 ] ) );
				s.clipId  = U64( f[ 3 ] );
				s.thumb   = f[ 4 ] == "-" ? std::string() : f[ 4 ];
				s.label   = rest;
				layout.lanes.back().steps.push_back( std::move( s ) );
			}
			continue;
		}
		if( line == "status not_found" )
			view.status = Status::NotFound;
		else if( line == "status ok" )
			view.status = Status::Ok;
		else if( line.rfind( "rev ", 0 ) == 0 )
			view.rev = line.substr( 4 );
		else if( line == "layout" )
			inLayout = true;
		else if( line == "run 0" )
			runtime.running = false;
		else if( line.rfind( "run 1 ", 0 ) == 0 && Fields( line, 7, f ) )
		{
			runtime.running   = true;
			runtime.pos       = U64( f[ 2 ] );
			runtime.passTicks = U64( f[ 3 ] );
			runtime.pass      = static_cast< uint32_t >( U64( f[ 4 ] ) );
			runtime.passes    = static_cast< uint32_t >( U64( f[ 5 ] ) );
			runtime.barBeat   = f[ 6 ];
		}
		else if( line.rfind( "lanestate ", 0 ) == 0 && Fields( line, 4, f ) )
			runtime.lanes.push_back( LaneState{ f[ 2 ], std::atoi( f[ 3 ].c_str() ) } );
		else if( line.rfind( "clock ", 0 ) == 0 && Fields( line, 3, f ) )
		{
			runtime.clockRunning = f[ 1 ] == "1";
			runtime.bpm          = std::atof( f[ 2 ].c_str() );
		}
		else if( line == "end" )
			gotEnd = true;
	}
	if( !gotEnd || inLayout )
	{
		view.status = Status::BadResponse;
		return false;
	}
	if( view.status == Status::NotFound )
	{
		view.hasLayout = false;
		view.rev.clear();
		view.runtime = Runtime{};
		return true;
	}
	if( gotLayout )
	{
		view.layout    = std::move( layout );
		view.hasLayout = true;
	}
	view.runtime = std::move( runtime );
	return true;
}

std::string UrlEncode( const std::string& s )
{
	static const char* hex = "0123456789ABCDEF";
	std::string out;
	for( unsigned char c : s )
	{
		if( ( c >= 'A' && c <= 'Z' ) || ( c >= 'a' && c <= 'z' ) || ( c >= '0' && c <= '9' ) || c == '-' || c == '_' || c == '.' || c == '~' )
			out += static_cast< char >( c );
		else
		{
			out += '%';
			out += hex[ c >> 4 ];
			out += hex[ c & 15 ];
		}
	}
	return out;
}
} // namespace seqview
