# tt init

Initialize a new town.

## Synopsis

```bash
tt init [OPTIONS]
```

## Description

Creates a new Tinytown workspace in the current directory. This:
1. Creates the directory structure (`agents/`, `logs/`, `tasks/`)
2. Generates `tinytown.json` configuration
3. Starts a Redis server with Unix socket
4. Verifies Redis 8.0+ is installed

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--name <NAME>` | `-n` | Town name (defaults to `<repo>-<branch>`) |
| `--town <PATH>` | `-t` | Town directory (defaults to `.`) |
| `--verbose` | `-v` | Enable verbose logging |

## Default Name

If `--name` is not provided, the town name is automatically derived from:

1. **Git repo + branch**: `<repo-name>-<branch-name>` (e.g., `redisearch-feature-auth`)
2. **Git repo only**: If no branch is available
3. **Directory name**: Fallback if not in a git repo

This makes it easy to have unique town names per feature branch.

## Examples

### Basic Initialization (Auto-Named)

```bash
cd ~/git/my-project
git checkout feature-auth
tt init
# Town name: my-project-feature-auth
```

### With Custom Name

```bash
tt init --name "My Awesome Project"
```

### Initialize in Different Directory

```bash
tt init --town ./projects/new-project --name new-project
```

## Output

```
✨ Initialized town 'my-project' at .
📡 Redis running with Unix socket for fast message passing
🚀 Run 'tt spawn <name>' to create agents
```

## Files Created

```
my-project/
├── tinytown.json     # Configuration
├── agents/           # Agent working directories
├── logs/             # Activity logs
└── tasks/            # Task storage
```

## Configuration

The generated `tinytown.json`:

```json
{
  "name": "my-project",
  "redis": {
    "use_socket": true,
    "socket_path": "redis.sock"
  },
  "models": {
    "claude": { "name": "claude", "command": "claude --print" },
    "auggie": { "name": "auggie", "command": "augment" },
    "codex": { "name": "codex", "command": "codex" }
  },
  "default_model": "claude",
  "max_agents": 10
}
```

## Errors

### Redis Not Found

```
Error: Redis not found. Please install Redis 8.0+ and ensure 'redis-server' is on your PATH.
See: https://redis.io/downloads/
```

**Solution:** Install Redis 8.0+ and add to PATH.

### Redis Version Too Old

```
Error: Redis version 7.4 is too old. Tinytown requires Redis 8.0 or later.
See: https://redis.io/downloads/
```

**Solution:** Upgrade to Redis 8.0+.

### Directory Already Initialized

If `tinytown.json` already exists, `init` will fail. Use `tt start` to connect to an existing town.

## See Also

- [tt start](./status.md) — Start an existing town
- [tt spawn](./spawn.md) — Create agents
- [Installation Guide](../getting-started/installation.md)

