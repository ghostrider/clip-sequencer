#pragma once
// Parameter indices of the SeqView plugin (also used by the tests). Order = order in Resolume.
enum ParamType : unsigned int
{
	PT_SEQUENCE, // FF_TYPE_TEXT: name of the sequence to show (set automatically by the sequencer, or typed)
	PT_BUILD,    // read-only text parameter that displays the build info
	PT_COUNT
};
