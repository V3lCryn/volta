# Known Issues / Compiler Limitations

Running log of real gaps found during development, to be fixed properly
rather than worked around permanently.

---

## Module-level let/const not visible across imports

**Found:** while building lib/fixed.vlt

**Symptom:** a module-level `let FX_ONE: i64 = 65536` (or `const` --
tested both, same result) declared at the top of lib/fixed.vlt is not
visible inside functions in that same file when the module is imported
by another file via `import "fixed"`. Every reference to the constant
fails with `undefined variable 'FX_ONE'`, even though the function using
it is defined in the same file as the constant.

**Root cause (suspected, not confirmed):** the import resolver appears
to inline/copy function definitions from the imported module into the
importing file's scope, but does not carry over top-level let/const
declarations from that module into the same scope the functions expect
to run in. So the functions get imported, but the module-level state
they depend on does not.

**Current workaround:** inline literal values directly into each
function body instead of referencing a shared module-level constant.
Used in lib/fixed.vlt -- every function repeats 65536 and 16 directly
rather than referencing FX_ONE / FX_SHIFT. This works but is fragile:
if the scale factor ever needs to change, it has to change in every
function individually instead of one place.

**Why this matters for the mission:** the whole point of Volta is
readability for domain experts. Having to inline magic numbers instead
of naming them (65536 instead of FX_ONE) is exactly the kind of
friction the language exists to remove. This should be fixed properly,
not left as a permanent pattern.

**Proposed fix direction:** when a module is imported, its top-level
let/const declarations should be hoisted into the importing file's
global scope alongside its functions -- the same way the functions
themselves currently get carried over. Needs investigation in the
import resolution logic (src/main.rs, resolve_import and whatever
calls it to actually merge the imported module's AST into the caller's).

**Status:** FIXED. `ast::Stmt::Const` was added to the cross-module
whitelist in `collect_stmts` (src/main.rs), so module-level `const`
declarations now correctly cross import boundaries, verified with
examples/const_import_test.vlt. lib/fixed.vlt has been refactored to
use real named constants (FX_SHIFT, FX_ONE) instead of the inlined
magic-number workaround, with identical output confirmed before and
after the change.

Note: plain `let` at module top-level intentionally still does NOT
cross the import boundary -- only `const` does. This is correct
behaviour, not a remaining bug: a mutable module-level `let` with
potential side effects shouldn't silently execute or leak state just
because something imports that file. Use `const` for any value meant
to be shared across modules.

---

## Ring buffer size must exceed the signal period it needs to capture

**Found:** while testing examples/anthem_bionics.vlt's ECG/cardiac monitor

**Symptom:** R-peak heart rate detection permanently returned bpm=0,
even though the EMG side of the same file worked correctly.

**Root cause:** the ring buffer (ECG_BUF_SIZE) was sized at 128 samples,
but the simulated heartbeat's R-R interval was 347 samples apart at the
chosen sample rate. Since the ring buffer only ever holds its most
recent N samples, it physically could not hold two heartbeats at once --
by the time the second beat arrived, the first had already been pushed
out and forgotten. The peak detector correctly found at most one peak
per window, and two peaks are required to compute an interval, so
bpm=0 forever, silently, with no error of any kind.

**Why this is a genuinely serious class of bug:** this is not a syntax
or type error -- the code compiled and ran without complaint. It is a
systems-level sizing mistake: a buffer sized without checking it against
the actual period of the real-world signal it needs to capture. In a
real embedded cardiac monitor, this exact mistake would silently report
"no heartbeat detected" forever -- one of the worst possible silent
failure modes a medical device could have.

**Fix:** ECG_BUF_SIZE increased from 128 to 1024, comfortably holding
several full heartbeat cycles.

**Lesson for future stdlib/example work:** whenever a ring buffer (or
any fixed-size buffer) is used to detect a periodic signal, always
explicitly size it to comfortably exceed the expected period of that
signal, and say so in a comment next to the buffer's declaration.

**Status:** fixed in examples/anthem_bionics.vlt (see commit history).

---

## Module-level import gap -- practical workaround pattern

Following on from the entry above: until the module-level let/const
import visibility gap is properly fixed, any new lib/ module should
follow the pattern used in lib/fixed.vlt -- inline literal values
directly inside each function body rather than referencing a shared
module-level constant, with a comment at the top of the file noting why.

---

## Silent crashes and silently-wrong results on invalid arithmetic -- fixed

**Found:** during a deliberate stress-testing session targeting edge
cases in lib/fixed.vlt and lib/filters.vlt.

**Symptom 1:** integer division or modulo by zero (e.g. `fx_div(a, 0)`,
or any plain `/`/`%` in user code) caused an immediate, silent process
crash with no error message -- a hardware-level SIGFPE, completely
opaque to whoever was running the program.

**Symptom 2:** `lpf_alpha(cutoff_hz, sample_rate)` with `cutoff_hz = 0`
silently returned `0.0` -- not because that's a correct answer, but
because the internal formula divides by `cutoff_hz`, producing a
floating-point `inf` that arithmetic then quietly resolved back down to
something plausible-looking. Separately, calling it with a cutoff at or
above the Nyquist limit (half the sample rate) -- a physically
meaningless request -- silently returned a plausible-looking number
instead of flagging that the request itself doesn't make sense.

**Why this matters, specifically for the mission:** in a real embedded
or bionic control loop, a momentary sensor glitch producing a zero
value, or a misconfigured cutoff/sample-rate pair, are both entirely
plausible real-world events -- not exotic edge cases. Before this fix,
either would have caused a silent crash or a silently wrong answer,
exactly the failure modes that are least acceptable in a device meant
to touch a real person.

**Fix:**
- Added `_vdiv`/`_vmod` runtime helpers that check for a zero divisor
  and print a clear `volta runtime error: division by zero` /
  `modulo by zero` message before exiting, instead of an opaque crash.
  All emitted `/` and `%` now route through these.
- Added explicit validation inside `lpf_alpha`: cutoff_hz and
  sample_rate_hz must both be positive, and cutoff_hz must be below
  the Nyquist limit (sample_rate/2) -- each violation now produces a
  specific, informative error message naming the actual values involved.

**Verified:** all existing tests (fixed_test.vlt, anthem_bionics.vlt)
produce byte-identical output to before the fix for every legitimate
input -- zero regressions. The new checks only trigger on genuinely
invalid input.

**Status:** FIXED.

**Note on design philosophy:** this is Option 3 from a three-option
analysis (silent fallback value / proper Result-based fallible
arithmetic / honest crash with a clear message). A proper Result-based
approach to fallible arithmetic, consistent with Volta's existing
Result/Ok/Err system, remains a legitimate longer-term improvement --
this fix trades some of that elegance for being small, safe, and
immediately deliverable without a larger design commitment.

---
