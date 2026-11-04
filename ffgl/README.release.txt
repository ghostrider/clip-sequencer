1000A Sequence - FFGL source plugin for Clip Sequencer for Resolume
Version {{VERSION}}

What it is
  A Resolume source plugin that shows a sequence of Clip Sequencer (lanes, steps with clip thumbnails,
  playhead, pass counter) on its placeholder clip on the SEQ layer. It only shows the sequence;
  starting and stopping works the same with or without it.

Installing
  Easiest: in Clip Sequencer, File > Install FFGL plugin... (with Resolume closed), or from a console:
    seq-app-cli.exe --install-ffgl
  By hand: copy SeqView.dll into Resolume's plugin folder, normally
    Documents\Resolume Arena\Extra Effects
  then start Resolume. It appears under Sources as "1000A Sequence".

Manual
  https://ghostrider.github.io/clip-sequencer/manual/ffgl-plugin.html

License
  MIT (see LICENSE). Uses the FFGL SDK, GLEW and stb; their licenses are in THIRD_PARTY_LICENSES.md.
