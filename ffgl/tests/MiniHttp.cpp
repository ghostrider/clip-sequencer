#include "MiniHttp.h"

#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <ws2tcpip.h>

#include <string>
#include <thread>

MiniHttp::MiniHttp( std::function< MiniResponse( const std::string& ) > h ) : handler( std::move( h ) ) {}

MiniHttp::~MiniHttp()
{
	{
		std::lock_guard< std::mutex > lock( mutex );
		stopping = true;
	}
	if( listener != ~0ull )
		closesocket( static_cast< SOCKET >( listener ) ); // unblocks accept()
	if( thread )
	{
		auto* t = static_cast< std::thread* >( thread );
		t->join();
		delete t;
	}
	WSACleanup();
}

bool MiniHttp::Start()
{
	WSADATA wsa;
	if( WSAStartup( MAKEWORD( 2, 2 ), &wsa ) != 0 )
		return false;
	SOCKET s = socket( AF_INET, SOCK_STREAM, IPPROTO_TCP );
	if( s == INVALID_SOCKET )
		return false;
	sockaddr_in addr{};
	addr.sin_family      = AF_INET;
	addr.sin_addr.s_addr = htonl( INADDR_LOOPBACK );
	addr.sin_port        = 0;
	if( bind( s, reinterpret_cast< sockaddr* >( &addr ), sizeof( addr ) ) != 0 || listen( s, 16 ) != 0 )
	{
		closesocket( s );
		return false;
	}
	int len = sizeof( addr );
	getsockname( s, reinterpret_cast< sockaddr* >( &addr ), &len );
	port     = ntohs( addr.sin_port );
	listener = static_cast< unsigned long long >( s );
	thread   = new std::thread( [ this ] { Run(); } );
	return true;
}

std::vector< std::string > MiniHttp::Requests()
{
	std::lock_guard< std::mutex > lock( mutex );
	return requests;
}

void MiniHttp::Run()
{
	for( ;; )
	{
		SOCKET c = accept( static_cast< SOCKET >( listener ), nullptr, nullptr );
		if( c == INVALID_SOCKET )
			return; // closed by the destructor
		std::string req;
		char buf[ 4096 ];
		while( req.find( "\r\n\r\n" ) == std::string::npos )
		{
			const int n = recv( c, buf, sizeof( buf ), 0 );
			if( n <= 0 )
				break;
			req.append( buf, static_cast< size_t >( n ) );
		}
		// "GET <path> HTTP/1.1"
		const size_t a = req.find( ' ' ), b = req.find( ' ', a + 1 );
		const std::string path = ( a != std::string::npos && b != std::string::npos ) ? req.substr( a + 1, b - a - 1 ) : "";
		{
			std::lock_guard< std::mutex > lock( mutex );
			requests.push_back( path );
		}
		const MiniResponse r = handler( path );
		const std::string head = "HTTP/1.1 " + std::to_string( r.status ) + ( r.status == 200 ? " OK" : " Error" ) +
		                         "\r\nContent-Type: " + r.contentType + "\r\nContent-Length: " + std::to_string( r.body.size() ) +
		                         "\r\nConnection: close\r\n\r\n";
		send( c, head.data(), static_cast< int >( head.size() ), 0 );
		send( c, r.body.data(), static_cast< int >( r.body.size() ), 0 );
		shutdown( c, SD_SEND );
		closesocket( c );
	}
}
