# Rust migration plan: rigcopy (C#)

1 packages, 50 source files (15377 lines), 0 test files, 0 UI files, grouped into 43 units.

| Ready | Caution | Blocked | Keep in source language |
| ---: | ---: | ---: | ---: |
| 17 | 26 | 0 | 0 |

## Start here

Best first ports: safe to port (readiness) and worth porting (loops and string work, tests, size).

| # | Unit | Lines | Priority | Readiness | Tests | Bridge | Main risks |
| ---: | --- | ---: | ---: | ---: | ---: | --- | --- |
| 1 | `Autostart.cs` | 251 | 48 | 73 | 0 | P/Invoke (C ABI) | Dynamic typing (11); Null references (7); Reflection and prototypes (1) |
| 2 | `Ak820Protocol.cs` | 308 | 44 | 56 | 0 | P/Invoke (C ABI) | Integer width and overflow (36); Dynamic typing (34); Division and modulo (27) |
| 3 | `KeyPaint.cs` | 253 | 41 | 59 | 0 | P/Invoke (C ABI) | Integer width and overflow (29); Division and modulo (16); Dynamic typing (10) |
| 4 | `Ak820Device.cs` | 380 | 41 | 53 | 0 | P/Invoke (C ABI) | Null references (12); Callback-shaped APIs (7); Concurrency (6) |
| 5 | `LightEffects.cs` | 259 | 38 | 51 | 0 | P/Invoke (C ABI) | Pointers and manual memory (3); Integer width and overflow (64); Division and modulo (23) |

### `Autostart.cs`

251 lines in 1 files, wave 0, priority 48, readiness 73, bridge **P/Invoke (C ABI)**.

- Leaf: imports no other unit.
- No tests found: write characterization tests against the original code first.
- Dynamic typing x11: `Autostart.cs:39`, `Autostart.cs:48`, `Autostart.cs:73`
- Null references x7: `Autostart.cs:39`, `Autostart.cs:48`, `Autostart.cs:73`
- Reflection and prototypes x1: `Autostart.cs:74`
- Callback-shaped APIs x1: `Autostart.cs:47`
- Exceptions x18: `Autostart.cs:44`, `Autostart.cs:51`, `Autostart.cs:70`

### `Ak820Protocol.cs`

308 lines in 1 files, wave 0, priority 44, readiness 56, bridge **P/Invoke (C ABI)**.

- Leaf: imports no other unit.
- No tests found: write characterization tests against the original code first.
- Integer width and overflow x36: `Ak820Protocol.cs:36`, `Ak820Protocol.cs:98`, `Ak820Protocol.cs:98`
- Dynamic typing x34: `Ak820Protocol.cs:35`, `Ak820Protocol.cs:36`, `Ak820Protocol.cs:45`
- Division and modulo x27: `Ak820Protocol.cs:69`, `Ak820Protocol.cs:75`, `Ak820Protocol.cs:96`
- Callback-shaped APIs x3: `Ak820Protocol.cs:197`, `Ak820Protocol.cs:198`, `Ak820Protocol.cs:362`
- Exceptions x2: `Ak820Protocol.cs:87`, `Ak820Protocol.cs:108`

### `KeyPaint.cs`

253 lines in 1 files, wave 0, priority 41, readiness 59, bridge **P/Invoke (C ABI)**.

- Leaf: imports no other unit.
- No tests found: write characterization tests against the original code first.
- Integer width and overflow x29: `KeyPaint.cs:135`, `KeyPaint.cs:151`, `KeyPaint.cs:157`
- Division and modulo x16: `KeyPaint.cs:132`, `KeyPaint.cs:133`, `KeyPaint.cs:134`
- Dynamic typing x10: `KeyPaint.cs:25`, `KeyPaint.cs:136`, `KeyPaint.cs:160`
- Callback-shaped APIs x6: `KeyPaint.cs:131`, `KeyPaint.cs:132`, `KeyPaint.cs:133`
- Null references x2: `KeyPaint.cs:160`, `KeyPaint.cs:197`

### `Ak820Device.cs`

380 lines in 1 files, wave 0, priority 41, readiness 53, bridge **P/Invoke (C ABI)**.

- Leaf: imports no other unit.
- No tests found: write characterization tests against the original code first.
- Null references x12: `Ak820Device.cs:47`, `Ak820Device.cs:56`, `Ak820Device.cs:56`
- Callback-shaped APIs x7: `Ak820Device.cs:53`, `Ak820Device.cs:54`, `Ak820Device.cs:63`
- Concurrency x6: `Ak820Device.cs:2`, `Ak820Device.cs:25`, `Ak820Device.cs:126`
- Division and modulo x4: `Ak820Device.cs:131`, `Ak820Device.cs:133`, `Ak820Device.cs:368`
- Dynamic typing x3: `Ak820Device.cs:56`, `Ak820Device.cs:56`, `Ak820Device.cs:399`
- Integer width and overflow x3: `Ak820Device.cs:104`, `Ak820Device.cs:135`, `Ak820Device.cs:381`
- Class inheritance x1: `Ak820Device.cs:10`
- Exceptions x16: `Ak820Device.cs:49`, `Ak820Device.cs:83`, `Ak820Device.cs:124`

### `LightEffects.cs`

259 lines in 1 files, wave 0, priority 38, readiness 51, bridge **P/Invoke (C ABI)**.

- Leaf: imports no other unit.
- No tests found: write characterization tests against the original code first.
- Pointers and manual memory x3: `LightEffects.cs:259`, `LightEffects.cs:260`, `LightEffects.cs:261`
- Integer width and overflow x64: `LightEffects.cs:42`, `LightEffects.cs:51`, `LightEffects.cs:53`
- Division and modulo x23: `LightEffects.cs:42`, `LightEffects.cs:44`, `LightEffects.cs:53`
- Dynamic typing x15: `LightEffects.cs:24`, `LightEffects.cs:178`, `LightEffects.cs:179`
- Callback-shaped APIs x1: `LightEffects.cs:181`
- Null references x1: `LightEffects.cs:120`

## Order by package

Packages come after the workspace packages they use. Inside a package, port by wave: a unit's imports are in earlier waves.

### 1. RigControl (`.`, portable, 0 test files)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `PumpLog.cs` | ready | 14 | 100 | P/Invoke (C ABI) | - |
| 0 | `AppFonts.cs` | caution | 26 | 97 | P/Invoke (C ABI) | Concurrency (1) |
| 0 | `TimerResolution.cs` | ready | 59 | 94 | P/Invoke (C ABI) | Dynamic typing (1); Exceptions (6) |
| 0 | `RigProfile.cs` | caution | 69 | 93 | P/Invoke (C ABI) | Null references (2); Dynamic typing (1) |
| 0 | `QuietLog.cs` | ready | 37 | 92 | P/Invoke (C ABI) | Reflection and prototypes (1); Integer width and overflow (1) |
| 0 | `Clock.cs` | ready | 12 | 91 | P/Invoke (C ABI) | Integer width and overflow (2); Division and modulo (2) |
| 0 | `LogFile.cs` | ready | 84 | 90 | P/Invoke (C ABI) | Concurrency (2); Integer width and overflow (1) |
| 0 | `Settings.cs` | caution | 114 | 88 | P/Invoke (C ABI) | Dynamic typing (5); Null references (1) |
| 0 | `Devices/` | caution | 98 | 85 | P/Invoke (C ABI) | Null references (4); Dynamic typing (2) |
| 0 | `App.xaml.cs` | ready | 69 | 83 | P/Invoke (C ABI) | Callback-shaped APIs (6); Dynamic typing (2) |
| 0 | `TrayIcon.cs` | ready | 242 | 74 | P/Invoke (C ABI) | Dynamic typing (7); Null references (4) |
| 0 | `Autostart.cs` | ready | 251 | 73 | P/Invoke (C ABI) | Dynamic typing (11); Null references (7) |
| 0 | `MainWindow.Memory.cs` | ready | 141 | 73 | P/Invoke (C ABI) | Callback-shaped APIs (7); Dynamic typing (3) |
| 0 | `ImageFrames.cs` | caution | 91 | 70 | P/Invoke (C ABI) | Integer width and overflow (13); Division and modulo (8) |
| 0 | `MainWindow.Battery.cs` | ready | 89 | 69 | P/Invoke (C ABI) | Null references (9); Dynamic typing (7) |
| 0 | `MainWindow.MouseRate.cs` | ready | 143 | 69 | P/Invoke (C ABI) | Dynamic typing (9); Integer width and overflow (6) |
| 0 | `FanCurveEditor.cs` | caution | 256 | 63 | P/Invoke (C ABI) | Division and modulo (8); Null references (7) |
| 0 | `GifFrames.cs` | caution | 171 | 61 | P/Invoke (C ABI) | Pointers and manual memory (2); Dynamic typing (16) |
| 0 | `KeyPaint.cs` | ready | 253 | 59 | P/Invoke (C ABI) | Integer width and overflow (29); Division and modulo (16) |
| 0 | `Ak820Protocol.cs` | ready | 308 | 56 | P/Invoke (C ABI) | Integer width and overflow (36); Dynamic typing (34) |
| 0 | `ColorWheel.cs` | caution | 206 | 56 | P/Invoke (C ABI) | Integer width and overflow (30); Division and modulo (26) |
| 0 | `ZenTuning.cs` | caution | 292 | 55 | P/Invoke (C ABI) | Null references (31); Integer width and overflow (23) |
| 0 | `Ak820Device.cs` | ready | 380 | 53 | P/Invoke (C ABI) | Null references (12); Callback-shaped APIs (7) |
| 0 | `Ak820Dongle.cs` | caution | 331 | 51 | P/Invoke (C ABI) | Pointers and manual memory (1); Dynamic typing (32) |
| 0 | `LightEffects.cs` | ready | 259 | 51 | P/Invoke (C ABI) | Pointers and manual memory (3); Integer width and overflow (64) |
| 0 | `GifLight.cs` | caution | 334 | 50 | P/Invoke (C ABI) | Integer width and overflow (62); Division and modulo (51) |
| 0 | `SystemInfo.cs` | caution | 241 | 45 | P/Invoke (C ABI) | Regex features the `regex` crate lacks (1); Hash iteration order (1) |
| 0 | `MainWindow.Fans.cs` | caution | 252 | 43 | P/Invoke (C ABI) | Hash iteration order (4); String offsets and slicing (3) |
| 0 | `MainWindow.Argb.cs` | caution | 479 | 40 | P/Invoke (C ABI) | Dynamic typing (38); Null references (18) |
| 0 | `MainWindow.System.cs` | caution | 388 | 40 | P/Invoke (C ABI) | Dynamic typing (47); Callback-shaped APIs (28) |
| 0 | `Mouse/` | caution | 448 | 37 | P/Invoke (C ABI) | Dynamic typing (66); Integer width and overflow (29) |
| 0 | `MainWindow.Profiles.cs` | caution | 482 | 35 | P/Invoke (C ABI) | Hash iteration order (6); String offsets and slicing (1) |
| 0 | `HidInterface.cs` | caution | 603 | 24 | P/Invoke (C ABI) | Pointers and manual memory (6); String offsets and slicing (2) |
| 0 | `LightMirror.cs` | caution | 371 | 23 | P/Invoke (C ABI) | Pointers and manual memory (3); Hash iteration order (2) |
| 0 | `MainWindow.KeyPaint.cs` | caution | 712 | 8 | P/Invoke (C ABI) | Pointers and manual memory (3); Hash iteration order (2) |
| 0 | `Argb/` | caution | 712 | 5 | P/Invoke (C ABI) | Pointers and manual memory (9); Integer width and overflow (83) |
| 0 | `Cooler/` | caution | 1590 | 0 | P/Invoke (C ABI) | String offsets and slicing (1); Hash iteration order (1) |
| 0 | `MainWindow.Cooler.cs` | caution | 1501 | 0 | P/Invoke (C ABI) | Pointers and manual memory (5); String offsets and slicing (2) |
| 0 | `MainWindow.xaml.cs` | caution | 1950 | 0 | P/Invoke (C ABI) | String offsets and slicing (1); Hash iteration order (1) |
| 1 | `MainWindow.Devices.cs` | ready | 195 | 64 | P/Invoke (C ABI) | Dynamic typing (32); Null references (26) |
| 1 | `MainWindow.MouseKeys.cs` | ready | 257 | 53 | P/Invoke (C ABI) | Dynamic typing (30); Null references (22) |
| 1 | `MainWindow.MouseDpi.cs` | caution | 299 | 48 | P/Invoke (C ABI) | Dynamic typing (24); Async and promises (12) |
| 1 | `MainWindow.Mouse.cs` | caution | 568 | 30 | P/Invoke (C ABI) | Dynamic typing (77); Null references (71) |

## NuGet dependencies

| Package | Units | Rust | Fit | Note |
| --- | ---: | --- | --- | --- |
| `LibreHardwareMonitor.Hardware` | 1 | ? | unknown |  |
| `RigControl.Mouse` | 1 | ? | unknown |  |

## Risk guide

- **String offsets and slicing** (High): JS indexes strings in UTF-16 code units, Rust in UTF-8 bytes. Offsets that cross the boundary must be converted, or non-ASCII text gets wrong ranges silently.
- **Regex features the `regex` crate lacks** (High): Lookaround, backreferences and the `y` flag are not in the `regex` crate. Use `fancy-regex` for those patterns, or rewrite them, and test them on non-ASCII text: `\w`, `\b` and case-insensitive matching differ between engines even where a pattern compiles. JS `/i` without `u` never matches a non-ASCII letter to an ASCII one, Rust `(?i)` does (`ſ` matches `s`, the Kelvin sign `K` matches `k`): write `(?i-u)` or compare with `eq_ignore_ascii_case`.
- **Dynamic typing** (Medium): Values whose shape is checked at runtime need an explicit Rust enum or `serde_json::Value`.
- **Exceptions** (Low): Thrown errors become `Result`. Errors crossing the bridge must map back to the same JS error shape.
- **Async and promises** (Medium): Promises become futures. Bridge with napi-rs `async fn` or `wasm-bindgen-futures`, or keep the async shell in JS and port the sync core.
- **Generators** (Medium): Lazy generators become iterators, or return a collected Vec across the bridge.
- **Class inheritance** (Medium): Class hierarchies become traits plus composition; `instanceof` checks become enums.
- **Reflection and prototypes** (Medium): Proxy, Reflect, Symbol and prototype tricks need a redesign, not a translation.
- **Callback-shaped APIs** (Medium): Function-typed parameters and methods cross the bridge on every call. Keep boundaries coarse: pass data in, get data out.
- **Integer width and overflow** (Medium): Python ints never overflow; C, Go and Java ints wrap or are undefined on overflow. Pick i64, i128 or a bignum crate deliberately, and use wrapping_* where the source wraps.
- **Division and modulo** (Medium): Python `//` and `%` floor toward negative infinity; Rust `/` and `%` truncate: use `div_euclid`/`rem_euclid` or explicit floor math. C division by zero is undefined.
- **Hash iteration order** (High): Set and map iteration order comes from the source language's hash function. A port cannot reproduce it: sort before output, or make the order part of the contract.
- **Pointers and manual memory** (High): Pointer arithmetic and manual memory become slices, indices and owned types. Code that exposes pointers across the boundary needs `unsafe`, isolated and explained.
- **Concurrency** (Medium): Threads, goroutines and locks need a design in Rust (channels, Send/Sync), not a line-by-line translation; keep concurrency in the caller where possible.
- **Null references** (Medium): Any reference may be null in the source: decide per parameter whether it is `Option` and what null means, instead of mapping every value to `Option`.
