# Installation

Getting Tinytown running takes about 30 seconds.

## Prerequisites

### Redis 8.0+

Tinytown requires Redis 8.0 or later.

#### Option 1: Bootstrap (Recommended) ⭐

Let Tinytown download and build Redis for you using an AI agent:

```bash
tt bootstrap
export PATH="$HOME/.tt/bin:$PATH"
```

This gets you the latest Redis compiled and optimized for your machine. Add the export to `~/.zshrc` or `~/.bashrc` for persistence.

#### Option 2: Package Manager

**macOS:**
```bash
brew install redis
```

**Ubuntu/Debian:**
```bash
curl -fsSL https://packages.redis.io/gpg | sudo gpg --dearmor -o /usr/share/keyrings/redis-archive-keyring.gpg
echo "deb [signed-by=/usr/share/keyrings/redis-archive-keyring.gpg] https://packages.redis.io/deb $(lsb_release -cs) main" | sudo tee /etc/apt/sources.list.d/redis.list
sudo apt-get update
sudo apt-get install redis
```

#### Option 3: From Source (Manual)

```bash
curl -LO https://github.com/redis/redis/archive/refs/tags/8.0.2.tar.gz
tar xzf 8.0.2.tar.gz
cd redis-8.0.2 && make && sudo make install
```

For more options, see the [Redis downloads page](https://redis.io/downloads/).

### Rust 1.85+

Tinytown is written in Rust. Install it via [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

## Install Tinytown

### From Source (Recommended)

```bash
git clone https://github.com/jeremyplichta/tinytown.git
cd tinytown
cargo install --path .
```

### From crates.io (Coming Soon)

```bash
cargo install tinytown
```

## Verify Installation

```bash
# Check tt is installed
tt --help

# Should output:
# Tinytown - Simple multi-agent orchestration using Redis
# ...

# Verify Redis version
redis-server --version
# Should show v=8.x.x or higher
```

## What's Next?

You're ready to go! Head to the [Quick Start](./quickstart.md) to create your first town.

