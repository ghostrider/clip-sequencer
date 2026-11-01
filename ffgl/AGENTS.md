# FFGL Plugin — Agent Instructions

**Read this whole file before touching any code in `ffgl/`, regardless of which model or tool you are.**

*(Taken over from the author's FFGL plugin collection, where this plugin was first developed. The incidents below
happened in other plugins there (DepthMap, GltfScene); the rules apply here unchanged.)*

This file exists because a past AI contribution to this codebase (DepthMap's "Pixel Cloud" feature, added by another
assistant) shipped broken, unverified work that sat undetected in the repo for an unknown period: a test project that
could never have compiled, a fabricated SDK struct field, an orphaned duplicate header, and a hardcoded dependency
path with no verification that the dependency was even present. None of it was caught because nothing was ever
actually built and run. Everything below is the discipline that would have caught it, written down so it doesn't
happen again.

## General Engineering Standards (every agent, every language, every task here)

### 1. Verify, don't assume — especially about APIs you didn't write
Never reference a struct field, function signature, enum value, or library behavior from memory or from a guess
at what seems reasonable. Open the actual header or source and read it.

**What went wrong, concretely:** a past change wrote `inputTexture.MaxUVs = {1.0f, 1.0f};` in a test file.
`FFGLTextureStruct` (see `external/ffgl/source/lib/ffgl/FFGL.h`) has no `MaxUVs` field — it has `Width`, `Height`,
`HardwareWidth`, `HardwareHeight`, `Handle`. The field was invented. The file could not compile, and apparently
nobody tried.
**Rule:** before using any struct member, enum value, or function argument from an SDK/library you're integrating
with, grep the actual header for it. If you can't find the declaration, you don't know the API yet — go read it.

### 2. Dependencies: vendor the whole chain, and verify it actually loads
A library linked at build time (`.lib` / import library) is not the same thing as the shared library needed at
*run time* (`.dll`). Both must be present, and the runtime one must end up next to whatever process loads it (or be
wired through the build's install/packaging step).

**What went wrong:** `DepthMap.vcxproj` links `onnxruntime.lib` and has a post-build step that copies
`onnxruntime.dll` next to the plugin — but `third_party/onnxruntime/lib/` only ever contained the `.lib`, never the
actual `.dll`. The plugin failed to load in Resolume with Windows error 126 ("module not found"). Because this
failure happens at the OS loader level, *none of the plugin's own code ever got a chance to log anything* — it just
silently vanished from the host's effects list, with zero diagnostic trace from inside the plugin. This went
unnoticed because nobody actually tried loading the built DLL in the real host after building it — a clean compile
was mistaken for a working plugin.
**Rule:** after adding or changing a dependency, actually load the artifact you built — run the test executable, or
check the real host's own log for a successful load — and watch it succeed. A clean compile proves nothing about
the runtime loadability of a dynamically-linked dependency.

### 3. "I added tests" is not the same as "I ran the tests"
A test file that was never compiled is not a test. A test that was compiled once, failed, and left a build-failure
marker sitting in the repo is a confession, not a safety net.

**What went wrong:** a test project (`DepthMapTests.vcxproj`) was added with (a) a wrong relative path to the FFGL
SDK sources, so none of them were ever compiled in, (b) no `glew.props` import, (c) a reference to a `ParamType`
enum that actually lived only in a different translation unit's file scope and was never visible to the test file,
and (d) the fabricated `MaxUVs` field from rule 1. It could not possibly have linked, let alone run. A stale
`*.tlog\unsuccessfulbuild` marker was still sitting in the repo as direct evidence nobody had gotten it to build.
Separately, even after being repaired enough to compile, it crashed with an access violation on its first real
render call, because `ProcessOpenGLStruct::inputTextures` is a `FFGLTextureStruct**` that the *caller* must
allocate as an array — not a fixed field you can point at one struct's address — another unverified assumption
about an API that was never actually read.
**Rule:** every test you write, you build and run yourself, in this same session, before calling the work done. If
it doesn't build, that IS the task — not a footnote to mention after the fact. If you genuinely cannot run it (no
GPU, no hardware, no credentials), say so explicitly and name exactly what's unverified. Never let silence imply a
test passed.

### 4. Weak assertions create false confidence — assert on the thing you actually changed
`CHECK(frame.lit > 0, "produces output")` passes whether or not the feature you just wrote does anything at all, as
long as *something* unrelated happens to be on screen. A depth-driven pixel-displacement effect with a check like
this will report success even if the displacement math is completely broken and displaces nothing.
**Rule:** if you add a displacement/transform effect, compare two frames and assert the output actually *moved* or
*changed in the specific way you intended* — don't just check that pixels exist. If you add a parameter, verify it
round-trips (`Set` then `Get` returns what you set) **and** that it visibly changes rendered output, not just one
or the other.

### 5. Thread and GL-context rules are invariants, not suggestions
In this codebase (and in most real-time plugin hosts): OpenGL calls are only valid on the thread currently holding
the GL context. A host can call `SetFloatParameter`/`SetTextParameter` from a different thread than the one that
renders. A destructor may run with **no GL context current at all** — several classes here say so explicitly in
their own comments.
**Rule:** before adding a GL call anywhere, trace which thread will actually execute it, under every call path that
reaches it — including destructors and parameter setters. If a parameter change needs to trigger GL work (e.g.
rebuilding a GPU resource for a newly selected asset), stage the request behind an atomic/flag and perform the
actual GL work only from the render callback (`ProcessOpenGL`), never inline inside the setter. Read the class's
own threading-model comment (most classes in this repo have one near the top) before changing anything that touches
more than one thread.

### 6. Don't duplicate a source of truth
If a parameter enum, config struct, or constant needs to be visible to more than one file, it belongs in exactly
one shared header, included everywhere it's needed. A second, independently-maintained copy of it *will* drift out
of sync with the first — it's not a hypothetical risk, it's a certainty given enough edits.
**Rule:** before adding a new file that looks like it duplicates something that already exists even partially,
check whether the existing thing can just be shared instead. If you do create a shared header for this purpose,
actually wire it into every consumer that needs it — don't leave the original inline copy in place "for now" (that
"for now" is how two copies silently drift apart).

### 7. Match this codebase's existing conventions before inventing your own
This repo already has established, working patterns for: parameter registration order (`Build` always registered
last), dropdown/option parameters (set and read as a raw element index, e.g. `0.0f`, `1.0f`, `2.0f` — never a
normalised `0..1` fraction), test harness structure (hidden WGL context, embedded PNG preview writer, a `Host`
struct owning a real FBO, a GPU timing sanity check, exporting `NvOptimusEnablement` so the test runs on the same
GPU the real host uses), and build script structure (resolve `vswhere.exe`/MSBuild by known install path since
neither is on `PATH`, a DLL-overwrite safety check before installing, deploying runtime DLL dependencies via a
post-build target).
**Rule:** before writing new code for a task you suspect has been done before in this repo, grep for how the
last two or three similar things were actually done, and match that — unless you have a specific, stated reason
not to.

### 8. Be proactive about verifying your own claims before reporting success
Don't say "this should work" — make it work, then show the evidence: a passing test run you actually executed, a
log line from the real host confirming it, a rendered image you actually looked at. Don't say "I added X" when X is
sitting uncompiled, untested, and unverified in the working tree. If something seems simple enough that you're
tempted to skip verifying it, that is exactly the moment to verify it anyway — the two real incidents documented in
this file were both "should be fine" assumptions that silently broke everything downstream of them.
**Rule:** the task is not done when the code is written. It's done when you've watched it build, watched it run,
and checked the actual output against what you expected — in this session, not left for someone else to discover
later wasn't true.

### Definition of done — run through this before saying a task is finished
- [ ] It builds clean, with zero new compiler warnings you introduced
- [ ] Every new or changed automated test actually ran, in this session, and you personally saw it pass
- [ ] Any new dependency's *runtime* artifacts (not just its link-time import library) are present where they're
      actually loaded from, and the resulting binary was actually exercised, not just compiled
- [ ] You checked the relevant real-world log/output for errors — not just the absence of a compiler error
- [ ] No fabricated API usage anywhere — every struct field and function call was confirmed against the real header
      or documentation, not recalled from memory
- [ ] No orphaned artifacts left behind: no unused duplicate headers, no broken/never-built test projects, no dead
      code paths "for later"

## Claude Code: load the detailed FFGL/Resolume skill too

If you are Claude Code specifically and the `ffgl-plugin-development` skill is available, also load it before creating or modifying any
plugin, debugging a build/render issue, or touching a PowerShell build script — it has the FFGL/Resolume-specific
technical pitfalls (parameter-registration gotchas that make a plugin silently never render, viewport-caching bugs,
PowerShell script validation, GPU-selection issues on dual-GPU laptops) that are too narrow for this general file
but are exactly as costly when missed.

## FFGL/Resolume-specific critical reminders (see the skill above for the full story on each)

- **PowerShell scripts must be syntactically valid** — validate with `[System.Management.Automation.Language.Parser]::ParseFile`, never by executing the script
- **No hidden/control characters** in text files (md, ps1, txt) — scan before committing
- **Viewport dimensions must be read fresh every frame** — `currentViewport` is only updated by `InitGL`/`Resize`, never automatically; caching it anywhere goes stale
- **Every `FF_TYPE_TEXT`/`FF_TYPE_FILE` parameter needs BOTH `GetTextParameter` AND `SetTextParameter` overridden**, the latter returning `FF_SUCCESS` for its own index — missing `SetTextParameter` makes a plugin load and register fine but never actually render, with no error anywhere
- **Unit tests are NOT sufficient on their own** — they must actually run on the same GPU the real host uses (export `NvOptimusEnablement`), and the plugin must still be confirmed in the actual host afterward
- **Resource files need proper headers** — include `<windows.h>` in `.rc` files
- **Every `SetParamInfof`/`SetParamInfo` name must be 16 ASCII characters or fewer, always** — the FreeFrame spec truncates longer names at the 16th character, silently. This isn't just cosmetic: if two different parameters share the same first 16 characters (e.g. `"Object Position X"` / `"Object Position Y"` / `"Object Position Z"`, each 17 chars, all truncating to the identical `"Object Position "`), the host sees multiple parameters with the *same displayed name* and gets confused about which one to automate/timeline - found live in GltfScene, where only the first of three identically-truncated rotation parameters could be added to Resolume's timeline. Count every parameter name before registering it (`grep`/`echo -n ... | wc -c`, or just count by hand) - don't estimate, a one-character overflow is exactly what caused this
