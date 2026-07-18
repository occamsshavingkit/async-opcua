# Quickstart: File Access Real I/O (FileType Open/Read/Write/Close)

## 1. Unit-level: registry and handler behavior

```sh
cargo test -p async-opcua-server --features fota --lib fota::file_access::
```

Expect: `Open`/`Close`/`Read`/`Write`/`GetPosition`/`SetPosition` handler unit tests pass,
including the open-conflict cases (`Bad_NotWritable`/`Bad_NotReadable`) and bounds checks
(`MaxByteStringLength`).

## 2. Real end-to-end: client writes, closes, reopens, reads back

```sh
cargo test -p async-opcua-server --features fota --test fota_file_access_integration -- --nocapture
```

Expect: a real client connects to a real server exposing a real FOTA `FileType` object, `Open`s
for write, `Write`s a known byte sequence, `Close`s, `Open`s for read, `Read`s the full content
back in a loop until an empty result (EOF), and the bytes match exactly.

## 3. Open-conflict enforcement

Same test file: a second client attempts to `Open` the same file for write while the first
client's write handle is still open; expect `Bad_NotWritable`. A third client attempts `Open` for
read while the write handle is open; expect `Bad_NotReadable`.

## 4. Fail-closed on adversarial input

Same test file: `Read` with a handle from a different session; `Read` with `length <= 0`; `Write`
with a payload larger than `MaxByteStringLength`; `SetPosition` far beyond the file's actual
size. Expect the documented status codes in every case, never a panic, never a hang.

## 5. Zero regression

```sh
cargo test -p async-opcua-server --all-features
```

Expect: full green, including the existing `fota::cleanup` and `fota::file_node` test suites
unchanged.
