# tt config

View or set global configuration.

## Synopsis

```bash
tt config [KEY] [VALUE]
```

## Description

Manages the global Tinytown configuration stored in `~/.tt/config.toml`. This configuration applies to all towns unless overridden.

## Arguments

| Argument | Description |
|----------|-------------|
| `KEY` | Config key to get or set (e.g., `default_cli`) |
| `VALUE` | Value to set (if omitted, shows current value) |

## Options

| Option | Short | Description |
|--------|-------|-------------|
| `--verbose` | `-v` | Enable verbose logging |

## Available Keys

| Key | Description | Values |
|-----|-------------|--------|
| `default_cli` | Default AI CLI for agents | `claude`, `auggie`, `codex`, `codex-mini`, `aider`, `gemini`, `copilot`, `cursor` |
| `agent_clis.<name>` | Custom CLI command for a named CLI | Any command string |

## Examples

### View All Configuration

```bash
tt config
```

Output:
```
⚙️  Global config: /Users/me/.tt/config.toml

default_cli = "claude"

[agent_clis]
my-custom = "custom-ai --mode agent"

Available CLIs: claude, auggie, codex, codex-mini, aider, gemini, copilot, cursor
```

### Get a Specific Value

```bash
tt config default_cli
```

Output:
```
claude
```

### Set Default CLI

```bash
tt config default_cli auggie
```

Output:
```
✅ Set default_cli = "auggie"
   Saved to: /Users/me/.tt/config.toml
```

### Add Custom CLI

```bash
tt config agent_clis.my-ai "my-ai-cli --flag"
```

## Configuration Precedence

1. CLI argument (`--cli`)
2. Town config (`tinytown.toml`)
3. Global config (`~/.tt/config.toml`)
4. Built-in default (`claude`)

## Config File Format

`~/.tt/config.toml`:
```toml
default_cli = "claude"

[agent_clis]
my-custom = "custom-ai --mode agent"
```

## See Also

- [Custom CLIs](../advanced/custom-models.md) — Adding custom AI CLIs
- [tt init](./init.md) — Town-level configuration
