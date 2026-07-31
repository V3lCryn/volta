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

**Status:** open, not yet fixed. Workaround in use.

---
