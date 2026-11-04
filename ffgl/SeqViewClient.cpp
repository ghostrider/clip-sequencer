#include "SeqViewClient.h"

// Own translation unit: needs the full Windows headers (FFGL.h defines NOUSER etc. and is not included here).
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <winhttp.h>

//stb_image: thumbnails are PNGs served by the sequencer. Decoded here, on the worker thread.
#define STB_IMAGE_IMPLEMENTATION
#define STBI_ONLY_PNG
#define STBI_FAILURE_USERMSG
#pragma warning( push )
#pragma warning( disable : 4244 4267 4996 4100 4456 4457 )
#include "stb_image.h"
#pragma warning( pop )

#include <set>

namespace seqview
{
namespace
{
std::wstring Widen( const std::string& s )
{
	if( s.empty() )
		return {};
	const int n = MultiByteToWideChar( CP_UTF8, 0, s.data(), static_cast< int >( s.size() ), nullptr, 0 );
	std::wstring w( static_cast< size_t >( n ), L'\0' );
	MultiByteToWideChar( CP_UTF8, 0, s.data(), static_cast< int >( s.size() ), w.data(), n );
	return w;
}

//! RAII for WinHTTP handles.
struct Handle
{
	HINTERNET h = nullptr;
	explicit Handle( HINTERNET handle ) : h( handle ) {}
	~Handle()
	{
		if( h )
			WinHttpCloseHandle( h );
	}
	Handle( const Handle& )            = delete;
	Handle& operator=( const Handle& ) = delete;
	explicit operator bool() const { return h != nullptr; }
};
} // namespace

bool HttpGet( const std::string& server, const std::string& path, std::string& body, int& status )
{
	status = 0;
	body.clear();
	const size_t colon = server.rfind( ':' );
	const std::string host = colon == std::string::npos ? server : server.substr( 0, colon );
	const int port         = colon == std::string::npos ? 80 : std::atoi( server.c_str() + colon + 1 );
	if( host.empty() || port <= 0 || port > 65535 )
		return false;

	Handle session( WinHttpOpen( L"SeqView/1", WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_NO_PROXY_NAME, WINHTTP_NO_PROXY_BYPASS, 0 ) );
	if( !session )
		return false;
	// Local server: fail fast instead of stalling the poll loop.
	WinHttpSetTimeouts( session.h, 500, 500, 500, 1000 );
	Handle connection( WinHttpConnect( session.h, Widen( host ).c_str(), static_cast< INTERNET_PORT >( port ), 0 ) );
	if( !connection )
		return false;
	Handle request( WinHttpOpenRequest( connection.h, L"GET", Widen( path ).c_str(), nullptr, WINHTTP_NO_REFERER,
	                                    WINHTTP_DEFAULT_ACCEPT_TYPES, 0 ) );
	if( !request )
		return false;
	if( !WinHttpSendRequest( request.h, WINHTTP_NO_ADDITIONAL_HEADERS, 0, WINHTTP_NO_REQUEST_DATA, 0, 0, 0 ) ||
	    !WinHttpReceiveResponse( request.h, nullptr ) )
		return false;
	DWORD code = 0, size = sizeof( code );
	if( !WinHttpQueryHeaders( request.h, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_HEADER_NAME_BY_INDEX,
	                          &code, &size, WINHTTP_NO_HEADER_INDEX ) )
		return false;
	status = static_cast< int >( code );
	for( ;; )
	{
		DWORD available = 0;
		if( !WinHttpQueryDataAvailable( request.h, &available ) )
			return false;
		if( available == 0 )
			break;
		const size_t old = body.size();
		body.resize( old + available );
		DWORD read = 0;
		if( !WinHttpReadData( request.h, body.data() + old, available, &read ) )
			return false;
		body.resize( old + read );
	}
	return true;
}

Client::~Client()
{
	Stop();
}

void Client::Start()
{
	std::lock_guard< std::mutex > lock( mutex );
	if( thread.joinable() )
		return;
	stopping = false;
	thread   = std::thread( [ this ] { Run(); } );
}

void Client::Stop()
{
	{
		std::lock_guard< std::mutex > lock( mutex );
		stopping = true;
	}
	wake.notify_all();
	if( thread.joinable() )
		thread.join();
}

void Client::SetTarget( const std::string& newServer, const std::string& newSequence )
{
	{
		std::lock_guard< std::mutex > lock( mutex );
		if( newServer == server && newSequence == sequence )
			return;
		server   = newServer;
		sequence = newSequence;
		++generation;
		view        = View{};
		view.status = sequence.empty() ? Status::NoName : Status::NoServer;
		viewChanged = true;
	}
	wake.notify_all();
}

void Client::SetActive( bool isActive )
{
	{
		std::lock_guard< std::mutex > lock( mutex );
		active = isActive;
	}
	wake.notify_all();
}

Update Client::Take()
{
	std::lock_guard< std::mutex > lock( mutex );
	Update u;
	if( viewChanged )
	{
		u.viewChanged = true;
		u.view        = view;
		u.receivedAt  = receivedAt;
		viewChanged   = false;
	}
	u.thumbnails.swap( ready );
	return u;
}

void Client::Inject( const View& v, std::vector< Thumbnail > thumbnails )
{
	std::lock_guard< std::mutex > lock( mutex );
	view        = v;
	viewChanged = true;
	receivedAt  = std::chrono::steady_clock::now();
	for( auto& t : thumbnails )
		ready.push_back( std::move( t ) );
}

void Client::Run()
{
	std::unique_lock< std::mutex > lock( mutex );
	while( !stopping )
	{
		if( active && !sequence.empty() && !server.empty() )
		{
			const std::string s = server, q = sequence;
			lock.unlock();
			PollOnce( s, q );
			lock.lock();
		}
		wake.wait_for( lock, kPollInterval, [ this ] { return stopping; } );
	}
}

void Client::PollOnce( const std::string& srv, const std::string& seq )
{
	uint64_t gen;
	std::string rev;
	View current;
	{
		std::lock_guard< std::mutex > lock( mutex );
		gen     = generation;
		rev     = view.rev;
		current = view;
	}
	// format=2: lengths in sixteenths (sequencer 0.2.0+); an older sequencer ignores it and answers SEQVIEW 1.
	std::string path = "/plugin/v1/view.txt?format=2&name=" + UrlEncode( seq );
	if( !rev.empty() && current.hasLayout )
		path += "&rev=" + UrlEncode( rev );
	std::string body;
	int status = 0;
	View next = current;
	if( !HttpGet( srv, path, body, status ) || status != 200 )
		next.status = Status::NoServer;
	else
		ParseView( body, next );

	// Thumbnails for clips of the (possibly new) layout, each version fetched once.
	std::vector< Thumbnail > decoded;
	if( next.status == Status::Ok && next.hasLayout )
	{
		std::set< uint64_t > wanted;
		for( const auto& lane : next.layout.lanes )
			for( const auto& step : lane.steps )
			{
				if( step.kind != StepKind::Clip || step.thumb.empty() || !wanted.insert( step.clipId ).second )
					continue;
				{
					std::lock_guard< std::mutex > lock( mutex );
					auto it = fetched.find( step.clipId );
					if( it != fetched.end() && it->second == step.thumb )
						continue;
					fetched[ step.clipId ] = step.thumb; // attempted; not retried until the version changes
				}
				std::string png;
				int code = 0;
				const std::string url = "/thumbs/clip/" + std::to_string( step.clipId ) + ".png?v=" + UrlEncode( step.thumb );
				if( !HttpGet( srv, url, png, code ) || code != 200 )
					continue;
				int w = 0, h = 0, channels = 0;
				stbi_uc* px = stbi_load_from_memory( reinterpret_cast< const stbi_uc* >( png.data() ), static_cast< int >( png.size() ), &w, &h, &channels, 4 );
				if( !px )
					continue;
				Thumbnail t;
				t.clipId  = step.clipId;
				t.version = step.thumb;
				t.width   = w;
				t.height  = h;
				t.rgba.assign( px, px + static_cast< size_t >( w ) * h * 4 );
				stbi_image_free( px );
				decoded.push_back( std::move( t ) );
			}
	}

	std::lock_guard< std::mutex > lock( mutex );
	if( gen != generation )
		return; // target changed meanwhile
	view        = std::move( next );
	viewChanged = true;
	receivedAt  = std::chrono::steady_clock::now();
	for( auto& t : decoded )
		ready.push_back( std::move( t ) );
}
} // namespace seqview
