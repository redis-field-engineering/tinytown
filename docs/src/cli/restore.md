# tt restore

Restore Redis state from AOF file.

## Synopsis

```bash
tt restore
```

## Description

Shows how to restore Redis state from a saved AOF file. This is useful when:
- Starting on a new machine
- Recovering from a crash
- Continuing work from a git checkout

## Example

```bash
tt restore
```

Output:
```
📂 AOF file found: ./redis.aof

   To restore from AOF:
   1. Stop Redis if running
   2. Start Redis with: redis-server --appendonly yes --appendfilename redis.aof
   3. Redis will replay the AOF and restore state

   Or just run 'tt init' - it will use existing AOF if present.
```

## Restore Workflow

### Option 1: Manual Restore

```bash
# Stop any running Redis
pkill redis-server

# Start Redis with AOF enabled
redis-server --appendonly yes --dir . --appendfilename redis.aof --port 0 --unixsocket redis.sock &

# Redis replays AOF and restores state
tt status
```

### Option 2: Fresh Init (Recommended)

If `redis.aof` exists in the town directory, `tt init` will automatically
configure Redis to use it:

```bash
cd my-project
tt init  # Detects existing redis.aof
tt status  # State is restored!
```

## What Gets Restored

- ✅ Agent registrations (names, states, models)
- ✅ Task states (pending, completed, etc.)
- ✅ Message queues (inbox contents)
- ✅ Activity logs (recent history)
- ✅ Stop flags, urgent queues, etc.

## See Also

- [tt save](./save.md) — Save state to AOF
- [Redis Configuration](../advanced/redis.md)

