# Volta Architecture Decisions

Real, deliberate architectural decisions made for Volta, written down
so the reasoning survives beyond a single conversation and doesn't
need to be re-derived later.

---

## Decision: native code generation (LLVM or similar) is the committed
## long-term compilation target -- not bundling a C compiler

**Date of decision:** early development, before any external users existed.

**The situation:** Volta currently compiles to C99 and then invokes the
system's installed C compiler (gcc/clang) to produce the final binary.
This works well today, but it means every person who wants to run a
Volta program needs a C compiler already installed on their machine --
straightforward on Linux/Mac, but a genuine, well-known pain point on
Windows, where no C compiler exists by default.

**The options considered:**

1. Bundle a small embeddable C compiler (e.g. TinyCC) invisibly inside
   the Volta installer, so end users never need to install anything
   themselves, while falling back to a real system gcc/clang when one
   is already present (for better optimization).

2. Give Volta its own native code generation backend -- either directly
   to machine code, or via LLVM (the same backend Rust and Swift use) --
   removing the C-compiler dependency entirely, for everyone, forever.

**Why option 2 was chosen, decisively, over option 1:**

Option 1 (bundled TinyCC) has a real, honest performance cost: TinyCC is
fast and tiny specifically because it deliberately skips most of the
optimization work a real compiler does. For most software this doesn't
matter. For Volta's actual mission -- real-time embedded and bionic
signal processing, where every microsecond of a control loop matters --
this is exactly the domain where that cost would hurt the most.

Option 2 has no such tradeoff. LLVM-class code generation is not a
compromise for accessibility -- it is strictly as good as or better than
what gcc/clang produce today. It solves the installation problem
completely, on every OS, permanently, while very likely making Volta's
own generated code faster, not slower.

The only reason to have considered option 1 at all was speed of
delivery -- getting to "install and run anywhere" sooner. But since
Volta currently has no external users depending on the current
architecture, there is no one who needs that shortcut, and no cost to
skipping straight to the correct long-term answer instead of building
a bridge that would later need to be partially undone.

**Decision:** commit to native code generation (LLVM or an equivalent
backend) as the actual, permanent architecture. Do not invest
engineering effort into bundling or shipping alongside an external C
compiler as a permanent solution.

---

## What this decision means for how Volta is built, starting now

This is not merely "revisit this later" -- it should shape decisions
made today, before the switch actually happens:

- **The AST must stay a clean, semantically rich representation of the
  program**, not something that quietly assumes it will become C text.
  If a language feature only makes sense because "the C emitter can
  special-case it," that's worth stopping and reconsidering.

- **The C emitter (src/emit.rs) is a means, not the destination.** It
  remains the practical way Volta produces working binaries today, but
  it should be mentally and structurally treated as a swappable backend
  stage sitting behind the AST, not as how Volta fundamentally works
  going forward.

- **The type checker (src/sema.rs) matters more than ever, not less.**
  A native codegen backend expects a fully, correctly typed program with
  no C compiler downstream to catch loose ends. Every real gap found and
  fixed in sema.rs is direct groundwork for this transition, not just
  routine bug fixing.

- **New language features should be evaluated against both present and
  future backends** where reasonably possible -- not blocking present
  work on a backend that doesn't exist yet, but staying aware that
  today's design choices are laying track for where the compiler is
  actually going.

**Status:** committed decision. Native codegen work has not yet begun --
current priority remains language completeness, stdlib depth, and real
hardware validation (see MISSION.md) using the existing C-emission
pipeline. This document exists so the eventual transition is a planned
destination, not a surprise rewrite.

---
