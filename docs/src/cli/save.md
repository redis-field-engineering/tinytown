# tt save

Save Redis state to AOF file for version control.

## Synopsis

```bash
tt save
```

## Description

Triggers Redis to compact and save its state to an AOF (Append Only File). This file can then be version controlled with git.

## How It Works

1. Sends `BGREWRITEAOF` command to Redis
2. Redis compacts all operations into a single AOF file
3. File is saved to `redis.aof` (configurable in `tinytown.json`)

## Example

```bash
tt save
```

Output:
```
💾 Saving Redis state...
   AOF rewrite triggered. File: ./redis.aof

   To version control Redis state:
   git add redis.aof
   git commit -m 'Save town state'
```

## Version Control Workflow

```bash
# Work on your project
tt spawn backend
tt assign backend "Build the API"
# ... agents work ...

# Save state before committing code
tt save
git add redis.aof tasks.toml
git commit -m "API implementation complete"

# Later, restore on another machine
git pull
tt restore  # See instructions
```

## AOF File Contents

The AOF file contains Redis commands to recreate state:
- All agent registrations
- All task states
- All messages in inboxes
- All activity logs

## Config Options

In `tinytown.json`:
```json
{
  "redis": {
    "persist": true,
    "aof_path": "redis.aof"
  }
}
```

## See Also

- [tt restore](./restore.md) — Restore state from AOF
- [tt sync](./sync.md) — Sync tasks.toml with Redis
- [Redis Configuration](../advanced/redis.md)

