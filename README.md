# ask

[![Build](https://github.com/CluEleSsUK/ask/actions/workflows/build.yml/badge.svg)](https://github.com/CluEleSsUK/ask/actions/workflows/build.yml)

A minimal CLI tool for querying a local [vLLM](https://github.com/vllm-project/vllm) instance (or any OpenAI-compatible API) from the terminal.

## Usage

```bash
# ask a question directly
ask "what is the meaning of life?"

# pipe input from another command
echo "explain this error" | ask

# specify a custom server URL
ask -u http://my-server:8000 "summarise this"

# use a specific model (auto-detected from the server by default)
ask -m llama-3 "hello"

# set a default server URL (saved to ~/.config/ask/config.json)
ask set-url http://my-server:8000

# subsequent calls will use the saved URL automatically
ask "hello"

# the -u flag still overrides the saved default
ask -u http://other-server:8000 "hello"
```

### Options

| Flag | Description | Default |
|------|-------------|---------|
| `-u, --url` | Base URL of the vLLM server | `http://localhost:8000` |
| `-m, --model` | Model name (fetched from server if omitted) | auto |
| `-r, --role` | Message role | `user` |

### Commands

| Command | Description |
|---------|-------------|
| `set-url <URL>` | Save a default server URL to `~/.config/ask/config.json` |

The URL resolution order is: `-u` flag > saved config > `http://localhost:8000`.

## Building from source

```bash
cargo build --release
```

The binary will be at `target/release/ask`.

## Releases

Pre-built binaries for Linux (x86_64) and macOS (x86_64, aarch64) are published as [GitHub releases](https://github.com/CluEleSsUK/ask/releases) on tagged versions.
