#pragma once
#include <chrono>
#include <mutex>
#include <string>

#include <FFGLSDK.h>

#include "SeqViewClient.h"
#include "SeqViewParams.h"
#include "SeqViewRender.h"

// SeqView ("1000A Sequence"): FFGL source that shows a sequence of the Resolume Clip Sequencer
// (vj_stuff/resolume-sequencer, SPEC §12) on the `§` placeholder clips: name, lanes with clip thumbnails, the
// playhead, bar.beat and pass while it runs. The sequencer sets the `Sequence` parameter by itself.
//
// Threading model:
//   - host threads call the constructor, Set/GetTextParameter, Connect/Disconnect;
//   - the render thread (GL context current) calls InitGL, ProcessOpenGL, Resize, DeInitGL;
//   - seqview::Client runs its own worker thread (HTTP + PNG decode) and hands results over by Take().
// GL objects live in seqview::Renderer and are only touched from InitGL/ProcessOpenGL/DeInitGL.
// Resolume creates short-lived instances (no InitGL) every few seconds; the worker only starts in InitGL and only
// polls between Connect and Disconnect, i.e. while the host actually renders this instance.
class SeqView : public CFFGLPlugin
{
public:
	SeqView();
	~SeqView() override;

	// CFFGLPlugin
	FFResult InitGL( const FFGLViewportStruct* vp ) override;
	FFResult ProcessOpenGL( ProcessOpenGLStruct* pGL ) override;
	FFResult DeInitGL() override;
	unsigned int Connect() override;
	unsigned int Disconnect() override;

	FFResult SetTextParameter( unsigned int dwIndex, const char* value ) override;
	char* GetTextParameter( unsigned int index ) override;

	// For tests.
	const seqview::View& CurrentView() const { return view; }
	size_t TextureCount() const { return renderer.TextureCount(); }
	bool FontReady() const { return renderer.FontReady(); }
	void InjectForTest( const seqview::View& v, std::vector< seqview::Thumbnail > thumbs ) { client.Inject( v, std::move( thumbs ) ); }

private:
	std::string Text( unsigned int index ) const;

	const int instance; // for telling instances apart in the host log
	mutable std::mutex textMutex;
	std::string sequence;
	std::string server = kDefaultServer;
	std::string textBuffer; // backing storage for GetTextParameter

	seqview::Client client;
	seqview::Renderer renderer;
	seqview::View view; // render thread only
	std::chrono::steady_clock::time_point receivedAt{};
	seqview::Status loggedStatus = seqview::Status::NoName;
};
