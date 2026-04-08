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

Volta compiles your `.vlt` file to C, runs clang/gcc on it, and immediately executes the result — all in one step. No separate compile step. The generated `.c` file is left next to your script if you want to inspect or modify it.

---

## Language

### Variables

```lua
let name = "Volta"
let x: i64 = 42
let pi: f64 = 3.14159
let flag: bool = true
```

### Functions

```lua
fn add(a: i64, b: i64) -> i64
  return a + b
end

fn greet(who: str) -> str
  return "Hello, " .. who .. "!"
end
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

for i in 0..10 do
  print(int_to_str(i))
end

for i in 1..=10 do
  print(int_to_str(i))
end
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

### Type casting

```lua
let n: i64 = 7
let f = n as f64
let half = f / 2.0    -- 3.5
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
| `i8` `i16` `i32` `i64` | `int8_t` to `int64_t` |
| `u8` `u16` `u32` `u64` | `uint8_t` to `uint64_t` |
| `f32` `f64` | `float` `double` |
| `bool` | `bool` |
| `str` | `const char*` |
| `ptr` | `void*` |

---

## Built-ins

### I/O
| Function | Description |
|----------|-------------|
| `print(s)` | Print string + newline |
| `input()` | Read line from stdin |

### Conversion
| Function | Description |
|----------|-------------|
| `int_to_str(n)` | Integer to string |
| `float_to_str(f)` | Float to string |
| `bool_to_str(b)` | Bool to "true" / "false" |
| `to_int(s)` | Parse string to integer |
| `to_float(s)` | Parse string to float |

### Strings
| Function | Description |
|----------|-------------|
| `str_len(s)` | Length in bytes |
| `str_eq(a, b)` | Equality check |
| `str_contains(s, sub)` | Substring check |
| `str_find(s, sub)` | Index of substring or -1 |
| `str_slice(s, start, len)` | Substring by position |
| `str_replace(s, from, to)` | Replace first occurrence |
| `char_at(s, i)` | Byte value at index |
| `char_from(n)` | Byte value to single char string |

### Security / low-level
| Function | Description |
|----------|-------------|
| `hex(n)` | Integer to hex string |
| `hex_dump(ptr, len)` | Dump raw bytes as hex |
| `bytes_to_hex(buf, len)` | Buffer to hex string |
| `xor_str(s, key)` | XOR string with byte key |
| `xor_bytes(buf, len, key)` | XOR buffer in-place |
| `rot13(s)` | ROT13 encode / decode |
| `caesar(s, shift)` | Caesar cipher |
| `hash_str(s)` | djb2 hash |
| `entropy(s)` | Shannon entropy 0.0 to 8.0 |
| `is_printable(c)` | Is byte printable ASCII |

### Math
| Function | Description |
|----------|-------------|
| `abs(n)` | Absolute value |
| `max(a, b)` | Maximum |
| `min(a, b)` | Minimum |
| `pow(base, exp)` | Integer power |
| `fsqrt(n)` | Square root |
| `ffloor(n)` / `fceil(n)` | Floor and ceiling |

### System
| Function | Description |
|----------|-------------|
| `arg_count()` | Number of CLI arguments |
| `arg_get(i)` | Get argument at index |
| `sleep_ms(n)` | Sleep N milliseconds |

---

## Cyber example

```lua
-- xor_demo.vlt
let payload = "Hello, Hacker!"
let key: i64 = 0x42

let encrypted = xor_str(payload, key)
let decrypted  = xor_str(encrypted, key)

print("Original:  " .. payload)
print("Key:       " .. hex(key))
print("Hash:      " .. int_to_str(hash_str(payload)))
print("Entropy:   " .. float_to_str(entropy(payload)))
print("Decrypted: " .. decrypted)
```

---

## Why Volta?

- **Feels like Lua** — clean syntax, no semicolons, end blocks, runs like a script
- **Compiles to C** — clang/gcc optimises your code, no GC, no VM, no interpreter overhead
- **Zero dependencies** — output binary links nothing unusual, drop it anywhere
- **C interop is first class** — @extern any C function in two lines
- **Hardware access built in** — @device maps memory-mapped registers by name
- **Security-aware stdlib** — XOR, entropy, hex dump, hashing built in from day one

---

## Roadmap

- [ ] Module system
- [ ] String interpolation
- [ ] Typed arrays
- [ ] First-class functions
- [ ] REPL
- [ ] Better error messages

---

## License

MIT
