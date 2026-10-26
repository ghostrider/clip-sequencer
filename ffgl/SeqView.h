#pragma once
#include <atomic>
#include <chrono>
#include <mutex>
#include <string>

#include <FFGLSDK.h>

#include "SeqViewParams.h"

// SeqView ("1000A Sequence"): FFGL source that shows a sequence of the Resolume Clip Sequencer
// (vj_stuff/resolume-sequencer, SPEC §12) on the `§` placeholder clips.
//
// Stage M11a (probe): renders a solid colour derived from the `Sequence` parameter and logs, per instance, every
// lifecycle call and how many frames it rendered per second. That answers what the sequencer design depends on:
// whether Resolume renders the source when its clip is only previewed, or connected on a layer with opacity 0.
//
// Threading: the host may call SetTextParameter from another thread than the one that renders; the sequence name is
// guarded by a mutex. All GL calls happen in ProcessOpenGL (render thread, context current).
class SeqView : public CFFGLPlugin
{
public:
	SeqView();
	~SeqView() override;

	// CFFGLPlugin
	FFResult InitGL( const FFGLViewportStruct* vp ) override;
	FFResult ProcessOpenGL( ProcessOpenGLStruct* pGL ) override;
	FFResult DeInitGL() override;
	unsigned int Resize( const FFGLViewportStruct* vp ) override;
	unsigned int Connect() override;
	unsigned int Disconnect() override;

	FFResult SetTextParameter( unsigned int dwIndex, const char* value ) override;
	char* GetTextParameter( unsigned int index ) override;

	std::string Sequence() const;

private:
	const int instance; // for telling instances apart in the host log
	mutable std::mutex sequenceMutex;
	std::string sequence;
	std::string textBuffer; // backing storage for GetTextParameter (render/host thread only)

	// Frame statistics for the once-per-second log line.
	std::chrono::steady_clock::time_point statsStart{};
	unsigned int framesSinceLog = 0;
};
