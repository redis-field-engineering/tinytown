# tt bootstrap

Download and build Redis using an AI coding agent.

## Synopsis

```bash
tt bootstrap [VERSION] [OPTIONS]
```

## Description

Bootstraps Redis by delegating to an AI coding agent. The agent:

1. Fetches the release info from https://github.com/redis/redis/releases
2. Downloads the source tarball
3. Builds Redis from source (`make`)
4. Installs binaries to `~/.tt/bin/`

This gets you the latest Redis compiled and optimized for your machine.

## Arguments

| Argument | Description |
|----------|-------------|
| `[VERSION]` | Redis version to install (default: `latest`) |

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--model <CLI>` | `-m` | AI CLI to use (default: `claude`) |

## Examples

### Install Latest Redis

```bash
tt bootstrap
```

### Install Specific Version

```bash
tt bootstrap 8.0.2
```

### Use Different AI CLI

```bash
tt bootstrap --model auggie
tt bootstrap --model codex
```

## Output

```
🚀 Bootstrapping Redis latest to /Users/you/.tt
   Using claude to download and build Redis...

📋 Running: claude --print --dangerously-skip-permissions < ~/.tt/bootstrap_prompt.md
   (This may take a few minutes to download and compile)

   [Agent output as it downloads and builds...]

✅ Redis installed successfully!

   Add to your PATH:
   export PATH="/Users/you/.tt/bin:$PATH"

   Or add to ~/.zshrc or ~/.bashrc for persistence.

   Then run: tt init
```

## After Bootstrap

Add Redis to your PATH:

```bash
# Add to current session
export PATH="$HOME/.tt/bin:$PATH"

# Add to shell permanently
echo 'export PATH="$HOME/.tt/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc

# Verify
redis-server --version
```

Then initialize a town:

```bash
tt init
```

## Why Bootstrap?

| Method | Pros | Cons |
|--------|------|------|
| **tt bootstrap** | Latest version, optimized for your CPU | Takes a few minutes to build |
| `brew install redis` | Quick, easy | May not have latest 8.0+ |
| `apt install redis` | System package | Often outdated version |

## Alternative Installation Methods

If bootstrap fails or you prefer package managers:

### macOS (Homebrew)

```bash
brew install redis
```

### Ubuntu/Debian

```bash
sudo apt update
sudo apt install redis-server
```

### From Source (Manual)

```bash
curl -LO https://github.com/redis/redis/archive/refs/tags/8.0.2.tar.gz
tar xzf 8.0.2.tar.gz
cd redis-8.0.2
make
sudo make install
```

## See Also

- [tt init](./init.md) — Initialize a town
- [Installation Guide](../getting-started/installation.md)

