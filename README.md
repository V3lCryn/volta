# Volta

A fast, low-level scripting language that compiles to C. Clean Lua-like syntax, native performance, zero runtime dependencies — built for people who work close to the metal.

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
- **Hardware access built in** — `@device` maps memory-mapped registers by name
- **Security-aware stdlib** — XOR, entropy, hex dump, hashing built in from day one

---

## Roadmap

- [ ] Generics / type-safe collections
- [ ] Interfaces / traits
- [ ] Module system
- [ ] Capturing closures
- [ ] Sorting built-in
- [ ] REPL
- [ ] LSP / IDE support

---

## License

MIT
