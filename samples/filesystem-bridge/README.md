# Filesystem Bridge

Mirrors a local filesystem directory as an OPC UA address space — directories become Object nodes, files become Variable nodes with their contents as values.

## Running

```bash
cargo run -- --root /tmp
```

Default root is `/tmp`. Pass `--root <path>` to mirror a different directory.

## How it works

On startup, the server recursively walks the specified root directory:
- **Directories** → Object nodes (folders you can browse into)
- **Files** → Variable nodes with contents as values (String for text, ByteString for binary)

Connect with any OPC UA client to browse and read the filesystem through OPC UA.
