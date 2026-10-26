#pragma once
// Worker thread that polls the sequencer (GET /plugin/v1/view.txt) and fetches + decodes clip thumbnails, so the
// render thread never waits for the network or a PNG decoder.
//
// Threading: Start/Stop/SetTarget/SetActive/Take are called from the plugin (host threads); everything else runs on
// the worker. Shared state lives behind one mutex. No GL here: decoded thumbnails are handed over as RGBA pixels and
// uploaded by the renderer on the render thread.
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <map>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include "SeqViewModel.h"

namespace seqview
{
struct Thumbnail
{
	uint64_t clipId = 0;
	std::string version;
	int width = 0, height = 0;
	std::vector< uint8_t > rgba; // top row first
};

//! What the render thread takes over each frame.
struct Update
{
	bool viewChanged = false;
	View view;
	std::chrono::steady_clock::time_point receivedAt{};
	std::vector< Thumbnail > thumbnails;
};

class Client
{
public:
	~Client();

	void Start();
	void Stop();
	//! Server "host:port" and sequence name. A change starts over (no revision, new status).
	void SetTarget( const std::string& server, const std::string& sequence );
	//! Polls only while the host renders this instance (between Connect and Disconnect).
	void SetActive( bool active );
	//! Moves new data (if any) out to the caller.
	Update Take();

	//! Test hook: deliver a view as if it had been fetched (no network needed).
	void Inject( const View& view, std::vector< Thumbnail > thumbnails );

	static constexpr std::chrono::milliseconds kPollInterval{ 50 };

private:
	void Run();
	void PollOnce( const std::string& server, const std::string& sequence );

	std::thread thread;
	std::mutex mutex;
	std::condition_variable wake;
	bool stopping = false;
	bool active   = false;
	std::string server, sequence;
	uint64_t generation = 0; // bumped by SetTarget; results of an older target are dropped

	// Worker-owned (also guarded by mutex when shared):
	View view;
	bool viewChanged = false;
	std::chrono::steady_clock::time_point receivedAt{};
	std::vector< Thumbnail > ready;
	std::map< uint64_t, std::string > fetched; // clip id -> version already fetched (or attempted)
};

//! Blocking HTTP GET (WinHTTP) of http://<server><path>. Returns false on any failure; `status` is the HTTP status.
bool HttpGet( const std::string& server, const std::string& path, std::string& body, int& status );
} // namespace seqview
