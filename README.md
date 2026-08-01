# Volta

A fast, low-level scripting language that compiles to C. Clean Lua-like syntax, native performance, zero runtime dependencies — built for people who work close to the metal.

**Volta exists to close the gap between domain expertise and hardware access** — so a biomedical engineer who understands nerve signals, or a security researcher who understands a binary format, doesn't need years of C mastery before they can express that understanding in real, working code that actually touches real hardware.

See [MISSION.md](MISSION.md) for why this project exists, and [ARCHITECTURE.md](ARCHITECTURE.md) for real engineering decisions and their reasoning, including the current C-emission pipeline and the committed path toward native code generation.

```lua
print("Hello, World!")
```

```bash
volta hello.vlt
# Hello, World!
```

Write it like a script. Runs like a native binary.

---

## Install

**Requirements:** Rust + clang or gcc

```bash
git clone https://github.com/V3lCryn/volta
cd volta
cargo install --path .
```

Now `volta` works from anywhere on your system.

---

## Usage

```bash
volta script.vlt
```

Volta compiles your `.vlt` file to C, runs clang/gcc on it, and immediately executes the result — all in one step. The generated `.c` file is left next to your script if you want to inspect or modify it.

---

## Language

### Variables

```lua
let name = "Volta"
let x: i64 = 42
let pi: f64 = 3.14159
let flag: bool = true
const MAX: i64 = 1024
type Fd = i64          -- type alias
```

### String interpolation

```lua
let user = "Alice"
let score: i64 = 99
print("Player {user} scored {score} points")
```

### Functions

```lua
fn add(a: i64, b: i64) -> i64
  return a + b
end

pub fn greet(who: str) -> str
  return "Hello, " .. who .. "!"
end
```

### Closures

```lua
let double = |x: i64| -> i64 do
  return x * 2
end

-- single-expression shorthand
let square = |x: i64| x * x
```

### Control flow

```lua
if x > 10 do
  print("big")
elseif x > 5 do
  print("medium")
else
  print("small")
end

while x > 0 do
  x -= 1
end

for i in 0..10 do        -- exclusive range
  print(int_to_str(i))
end

for i in 1..=10 do       -- inclusive range
  print(int_to_str(i))
end

for i, v in items do     -- index + value
  print(int_to_str(i) .. ": " .. int_to_str(v))
end
```

### Match

```lua
-- enum variants
match color do
  Color.Red   => print("red")
  Color.Green => print("green")
  _           => print("other")
end

-- integers and ranges
match code do
  200       => print("ok")
  301..302  => print("redirect")
  400..=499 => print("client error")
  500       => print("server error")
  _         => print("unknown")
end

-- strings
match method do
  "GET"  => handle_get()
  "POST" => handle_post()
  _      => print("unsupported")
end
```

### Arrays

```lua
let nums: [i64] = [1, 2, 3]
push(nums, 4)
let n = nums[0]
let total = arr_len(nums)

let words: [str] = ["hello", "world"]
push(words, "volta")
```

### Structs

```lua
struct Point
  x: i64
  y: i64
end

let p = Point { x: 10, y: 20 }
print(int_to_str(p.x))
p.y = 99
```

### Packed structs (bit fields)

```lua
packed struct Flags: u8
  active:   1
  mode:     2
  priority: 5
end
```

### Enums

```lua
enum Direction
  North
  South
  East
  West
end

let d = Direction.North
```

### Hash maps

```lua
let m: map = map_new()
map_set(m, "host", "localhost")
map_set(m, "port", 8080)

let host: str = map_get_str(m, "host")
let port: i64 = map_get_int(m, "port")

if map_has(m, "port") do
  map_del(m, "port")
end

let keys: [str] = map_keys(m)
print(int_to_str(map_len(m)))
map_free(m)
```

### Error handling

```lua
fn divide(a: i64, b: i64) -> Result
  if b == 0 do
    return Err("division by zero")
  end
  return Ok(a / b)
end

let r = divide(10, 2)
let val = r.unwrap()         -- exits on error
```

### Defer

```lua
fn read_file(path: str) -> str
  let f = open(path)
  defer close(f)             -- runs at function exit, LIFO
  return read(f)
end
```

### Memory management

```lua
-- manual heap allocation
let p: *i64 = alloc(8)
*p = 42
free(p)

-- arena bump allocator (no per-alloc free)
let arena = arena_new(4096)
let buf: *u8 = arena_alloc(arena, 256)
arena_reset(arena)
arena_free_all(arena)
```

### Pointers

```lua
let x: i64 = 10
let p: *i64 = &x
*p = 99
print(int_to_str(x))   -- 99
```

### Type casting

```lua
let n: i64 = 7
let f = n as f64
let half = f / 2.0     -- 3.5
```

### Bitwise operators

```lua
let flags: i64 = 0xFF
let lo    = flags & 0x0F
let hi    = (flags >> 4) & 0x0F
let xored = flags ^ 0x01
```

### C FFI

```lua
@extern "C" do
  fn system(cmd: str) -> i32
  fn getenv(name: str) -> str
end

let shell = getenv("SHELL")
print("Shell: " .. shell)
```

### Hardware registers

```lua
@device "gpio" at 0x40020000 do
  reg MODER: u32
  reg ODR:   u32
end

GPIO_MODER = 0x55
GPIO_ODR   = 0xFF
```

---

## Types

| Volta | C |
|-------|---|
| `i8` `i16` `i32` `i64` | `int8_t` → `int64_t` |
| `u8` `u16` `u32` `u64` | `uint8_t` → `uint64_t` |
| `f32` `f64` | `float` `double` |
| `bool` | `bool` |
| `str` | `const char*` |
| `ptr` | `void*` |
| `*T` | `T*` |
| `[T]` | `VArray` (dynamic) |
| `map` | `VMap` (string-keyed hash map) |
| `Result` | `VResult` |

---

## Built-ins

### I/O
| Function | Description |
|----------|-------------|
| `print(s)` | Print string + newline |
| `input()` | Read line from stdin |
| `arg_count()` | Number of CLI arguments |
| `arg_get(i)` | Argument at index |
| `sleep_ms(n)` | Sleep N milliseconds |

### Conversion
| Function | Description |
|----------|-------------|
| `int_to_str(n)` | Integer to string |
| `float_to_str(f)` | Float to string |
| `bool_to_str(b)` | Bool to `"true"` / `"false"` |
| `to_int(s)` | Parse string to integer |
| `to_float(s)` | Parse string to float |
| `str(x)` | Coerce value to string |

### Strings
| Function | Description |
|----------|-------------|
| `str_len(s)` | Length in bytes |
| `str_eq(a, b)` | Equality check |
| `str_contains(s, sub)` | Substring check |
| `str_find(s, sub)` | Index of substring or -1 |
| `str_slice(s, start, len)` | Substring by position |
| `str_replace(s, from, to)` | Replace first occurrence |
| `str_upper(s)` / `str_lower(s)` | Case conversion |
| `str_trim(s)` | Strip leading/trailing whitespace |
| `str_starts_with(s, p)` | Prefix check |
| `str_ends_with(s, x)` | Suffix check |
| `str_split(s, delim)` | Split into `[str]` array |
| `str_join(arr, delim)` | Join `[str]` array into string |
| `str_repeat(s, n)` | Repeat string N times |
| `char_at(s, i)` | Byte value at index |
| `char_from(n)` | Byte value to single-char string |

### Arrays
| Function | Description |
|----------|-------------|
| `push(arr, val)` | Append element |
| `pop(arr)` | Remove and return last element |
| `arr_len(arr)` | Number of elements |

### Hash maps
| Function | Description |
|----------|-------------|
| `map_new()` | Create empty map |
| `map_set(m, key, val)` | Insert or update |
| `map_get_int(m, key)` | Get integer value |
| `map_get_str(m, key)` | Get string value |
| `map_has(m, key)` | Key existence check |
| `map_del(m, key)` | Delete entry |
| `map_len(m)` | Number of entries |
| `map_keys(m)` | All keys as `[str]` |
| `map_free(m)` | Release memory |

### Math
| Function | Description |
|----------|-------------|
| `abs(n)` | Absolute value |
| `max(a, b)` / `min(a, b)` | Max / min |
| `pow(base, exp)` | Integer power |
| `fsqrt(n)` | Square root |
| `ffloor(n)` / `fceil(n)` | Floor / ceiling |

### File I/O
| Function | Description |
|----------|-------------|
| `file_read(path)` | Read entire file as string |
| `file_write(path, data)` | Write string to file |
| `file_append(path, data)` | Append string to file |
| `file_exists(path)` | Check file existence |
| `file_delete(path)` | Delete file |
| `file_size(path)` | File size in bytes |
| `file_readline(path, n)` | Read line N from file |

### TCP networking
| Function | Description |
|----------|-------------|
| `tcp_connect(host, port)` | Connect to host, returns fd |
| `tcp_listen(port)` | Bind + listen, returns fd |
| `tcp_accept(fd)` | Accept connection |
| `tcp_send(fd, data)` | Send string |
| `tcp_recv(fd)` | Receive into string |
| `tcp_recv_line(fd)` | Receive one line |
| `tcp_close(fd)` | Close connection |
| `tcp_ok(fd)` | Check fd is valid |
| `tcp_peer_ip(fd)` | Remote IP as string |

### Security / low-level
| Function | Description |
|----------|-------------|
| `hex(n)` | Integer to hex string |
| `hex_dump(ptr, len)` | Dump raw bytes |
| `bytes_to_hex(buf, len)` | Buffer to hex string |
| `xor_str(s, key)` | XOR string with byte key |
| `xor_bytes(buf, len, key)` | XOR buffer in-place |
| `rot13(s)` | ROT13 |
| `caesar(s, shift)` | Caesar cipher |
| `hash_str(s)` | djb2 hash |
| `entropy(s)` | Shannon entropy (0.0 – 8.0) |

### Signal processing (`lib/fixed.vlt`, `lib/filters.vlt`)

Built specifically for real-time embedded and bionic signal processing —
sensor sampling, filtering, and control loops on hardware that may not
even have a floating-point unit.

```lua
import "fixed"
import "filters"

-- Fixed-point math (Q16.16) — pure integer arithmetic, no FPU required
let a = fx_from_int(10)
let b = fx_from_float(3.5)
print(fx_to_str(fx_mul(a, b)))   -- 35.00000

-- Ring buffer for streaming sensor samples
let ring: VRing = ring_new(256)
ring_push(ring, raw_sample)

-- Composable filters built on top of Volta's DSP primitives
let smoothed = moving_average(ring)
let filtered = lowpass_filter(prev, sample, 10.0, 1000.0)  -- 10Hz cutoff @ 1kHz sample rate
let is_event = debounce_above(ring, threshold, 3)           -- reject single-sample noise
```

See `examples/anthem_bionics.vlt` for a full simulated EMG/ECG signal
pipeline — contraction scoring, servo mapping, R-peak heart rate
detection — built entirely from these primitives.

### Binary analysis / memory inspection

Low-level primitives for reverse engineering, security research, and
hardware register inspection.

| Function | Description |
|----------|-------------|
| `file_read_bin(path)` | Binary-safe file read, returns raw pointer |
| `buf_entropy(buf, len)` | Shannon entropy on raw bytes (handles nulls) |
| `strings_extract(buf, len, min_len)` | Extract printable ASCII runs from a binary buffer |
| `buf_find_str(buf, len, pattern)` | Search for a byte pattern in a binary buffer |
| `pe_is_valid(buf)` / `elf_is_valid(buf)` | Detect PE / ELF executable format |
| `pe_entry_point(buf)` / `elf_entry_point(buf)` | Read entry point address from header |
| `elf_arch_name(buf)` | Human-readable architecture string |
| `mem_dump(ptr, len)` | Hex + ASCII memory dump with address labels |
| `mem_scan(ptr, len, pattern)` | Hex-pattern search in memory |
| `mem_diff(a, b, len)` | Byte-level diff between two memory regions |
| `ptr_add(p, n)` / `ptr_to_int(p)` / `int_to_ptr(n)` | Pointer arithmetic |

See `examples/bin_triage.vlt` for a full stage-1 binary triage tool —
format detection, header parsing, entropy scanning, string extraction,
and XOR obfuscation detection, designed to run before opening a
disassembler.

### PostgreSQL

```lua
let ok = pg_connect(connstr)
let rows = pg_query("SELECT username FROM users WHERE active = true")
let i: i64 = 0
while i < rows do
  print(pg_value(i, 0))
  i += 1
end
pg_free()
pg_close()
```

---

## Bionics and embedded — the flagship example

`examples/anthem_bionics.vlt` simulates a real signal-processing
pipeline for a prosthetic limb controller, built entirely from Volta's
signal processing primitives:

- EMG (electromyography) sensor sampling via a ring buffer
- Real-time low-pass filtering with a physically meaningful cutoff
  frequency (not a hand-picked constant) -> contraction scoring ->
  servo PWM mapping for a prosthetic hand's grip
- Cardiac monitor: R-peak detection from a simulated ADC stream,
  producing accurate real-time BPM
- @critical block for atomic sensor reads
- @interrupt fn stub for a real ADC interrupt service routine pattern
- Hardware register map comments showing exactly what the real
  @device block would look like on actual ARM Cortex-M4 silicon

volta examples/anthem_bionics.vlt

[EMG] t=1050ms  score=100  ema=931.764
[EMG] Peak contraction score: 100
[SERVO] Mapping EMG score to prosthetic hand position:
  score=100  grip_pw=2000us  extend_pw=1000us
[ECG] Cardiac monitor -- simulating 10s of ECG @ 500Hz
  t=1s  bpm=86
  t=9s  bpm=86

This is currently a simulation -- I/O uses print() where a real
build would use @device register access. Getting Volta to run this
kind of pipeline on real embedded hardware is the current top-priority
milestone. See MISSION.md for why this matters.

---

## Example — TCP echo server

```lua
let srv = tcp_listen(9000)
print("listening on :9000")

while true do
  let fd = tcp_accept(srv)
  let msg = tcp_recv(fd)
  tcp_send(fd, "echo: " .. msg)
  tcp_close(fd)
end
```

## Example — security tool

```lua
let payload = "Hello, Hacker!"
let key: i64 = 0x42

let enc = xor_str(payload, key)
let dec = xor_str(enc, key)

print("Original:  " .. payload)
print("Key:       " .. hex(key))
print("Hash:      " .. int_to_str(hash_str(payload)))
print("Entropy:   " .. float_to_str(entropy(payload)))
print("Decrypted: " .. dec)
```

## Example — hash map word count

```lua
let text = "the cat sat on the mat the cat"
let words: [str] = str_split(text, " ")
let counts: map = map_new()

for i, w in words do
  if map_has(counts, w) do
    map_set(counts, w, map_get_int(counts, w) + 1)
  else
    map_set(counts, w, 1)
  end
end

let keys: [str] = map_keys(counts)
for i, k in keys do
  print(k .. ": " .. int_to_str(map_get_int(counts, k)))
end

map_free(counts)
```

---

## Why Volta?

- **Feels like Lua** — clean syntax, no semicolons, `end` blocks, runs like a script
- **Compiles to C** — clang/gcc optimises your code, no GC, no VM, no interpreter overhead
- **Zero dependencies** — output binary links nothing unusual, drop it anywhere
- **C interop is first class** — `@extern` any C function in two lines
- **Hardware access built in** — `@device` maps memory-mapped registers by name, `@critical`/`@interrupt` for real embedded control flow
- **Security-aware stdlib** — XOR, entropy, hex dump, hashing, PE/ELF parsing built in from day one
- **Signal-processing-aware stdlib** — fixed-point math, ring buffers, and composable filters, built for real-time embedded and bionic control loops

Most languages force a choice: readable and safe (Python, Lua) or fast
and hardware-capable (C). Volta is built specifically to remove that
choice — not by making hardware programming easy, some of it is
genuinely hard and always will be, but by removing the friction that
has nothing to do with the actual problem and everything to do with
decades-old language design. See [MISSION.md](MISSION.md).

---

## Roadmap

**Done:**
- [x] Module system (`import`)
- [x] Closures
- [x] LSP server + VS Code extension
- [x] Enums, pattern matching (`match`)
- [x] Result type + error handling
- [x] Packed structs, pointer types, `@critical` / `@interrupt` hardware primitives
- [x] Fixed-point arithmetic (`lib/fixed.vlt`) for FPU-less embedded targets
- [x] Ring buffers (`VRing`), signal filters (`lib/filters.vlt`) — moving average, EMA, low-pass, debounce, median filter
- [x] Low-level memory inspection API (`mem_dump`, `mem_scan`, `mem_diff`, pointer arithmetic)
- [x] Binary analysis toolkit — PE/ELF header parsing, entropy analysis, string extraction, XOR key detection

**Not yet:**
- [ ] Generics / type-safe collections
- [ ] Interfaces / traits
- [ ] Sorting built-in
- [ ] REPL
- [ ] Real hardware validation (currently simulated — see `examples/anthem_bionics.vlt`)
- [ ] Cross-platform installers (see [ARCHITECTURE.md](ARCHITECTURE.md))
- [ ] Native code generation via LLVM (committed long-term direction, see [ARCHITECTURE.md](ARCHITECTURE.md))

---

## License

MIT
