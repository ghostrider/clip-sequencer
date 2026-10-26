#pragma once
// What the sequencer reports about one sequence (GET /plugin/v1/view.txt, format documented in
// resolume-sequencer/crates/seq-app/src/plugin_view.rs). Plain data + a parser: no GL, no network, no threads.
#include <cstdint>
#include <string>
#include <vector>

namespace seqview
{
enum class StepKind
{
	Clip,
	Rest,
	Unresolved
};

struct Step
{
	StepKind kind = StepKind::Rest;
	uint32_t eighths = 1;
	uint64_t clipId = 0;
	std::string thumb; // thumbnail version, empty = none
	std::string label;
};

struct Lane
{
	std::string state = "ok"; // ok | missing | ambiguous | seq_layer
	std::string end;          // loop_to_end | hold | clear
	std::string layer;
	std::vector< Step > steps;
};

struct Layout
{
	std::string name, deck;
	uint32_t passEighths = 0;
	std::vector< Lane > lanes;
};

struct LaneState
{
	std::string state; // idle | playing | yielded | resume_pending | taken_over
	int step = -1;
};

struct Runtime
{
	bool running = false;
	uint64_t pos = 0, passTicks = 0;
	uint32_t pass = 0, passes = 0; // passes 0 = infinite
	std::string barBeat;
	std::vector< LaneState > lanes;
	bool clockRunning = false;
	double bpm = 0;
};

enum class Status
{
	NoName,      // no sequence assigned to this clip yet
	NoServer,    // the sequencer is not reachable
	NotFound,    // the sequencer has no sequence of that name
	BadResponse, // something answered, but not in the expected format
	Ok
};

struct View
{
	Status status = Status::NoName;
	std::string rev;    // layout revision; send it back so an unchanged layout is not resent
	bool hasLayout = false;
	Layout layout;
	Runtime runtime;
};

//! Parses a response into `view`. A response without a layout block keeps the previous layout (same revision).
//! Returns false (and sets Status::BadResponse) if the text is not a SEQVIEW 1 response.
bool ParseView( const std::string& text, View& view );

//! Percent-encodes a query parameter value (UTF-8 bytes).
std::string UrlEncode( const std::string& s );
} // namespace seqview
