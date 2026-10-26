#pragma once
// Tiny HTTP/1.1 test server on 127.0.0.1 (own translation unit: Winsock headers clash with FFGL.h's NOUSER etc.).
// Serves fixed responses by path prefix and records every requested path.
#include <functional>
#include <mutex>
#include <string>
#include <vector>

struct MiniResponse
{
	int status = 200;
	std::string contentType = "text/plain";
	std::string body;
};

class MiniHttp
{
public:
	//! `handler` gets the full path including the query string.
	explicit MiniHttp( std::function< MiniResponse( const std::string& path ) > handler );
	~MiniHttp();
	bool Start(); // binds an ephemeral port
	int Port() const { return port; }
	std::vector< std::string > Requests();

private:
	void Run();
	std::function< MiniResponse( const std::string& ) > handler;
	unsigned long long listener = ~0ull; // SOCKET
	int port = 0;
	void* thread = nullptr; // std::thread*, hidden from this header
	std::mutex mutex;
	std::vector< std::string > requests;
	bool stopping = false;
};
