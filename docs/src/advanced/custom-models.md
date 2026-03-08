# Custom Models

Add your own AI model configurations to Tinytown.

## Model Configuration

Models are defined in `tinytown.toml`:

```toml
[agent_clis.claude]
name = "claude"
command = "claude --print"

[agent_clis.my-custom-model]
name = "my-custom-model"
command = "/path/to/my/agent --config ./agent.yaml"
workdir = "/path/to/working/dir"

[agent_clis.my-custom-model.env]
API_KEY = "secret"
MODEL_VERSION = "v2"
```

## Model Properties

| Property | Required | Description |
|----------|----------|-------------|
| `name` | Yes | Identifier for `--model` flag |
| `command` | Yes | Shell command to run the agent |
| `workdir` | No | Working directory for the command |
| `env` | No | Environment variables |

## Example: Local LLM

```toml
[agent_clis.local-llama]
name = "local-llama"
command = "llama-cli --model llama-3-70b --prompt-file task.txt"
workdir = "~/.local/share/llama"

[agent_clis.local-llama.env]
CUDA_VISIBLE_DEVICES = "0"
```

Usage:
```bash
tt spawn worker-1 --model local-llama
```

## Example: Custom Script

Create a wrapper script:

```bash
#!/bin/bash
# ~/bin/my-agent.sh

# Read task from stdin or argument
TASK="$1"

# Your custom agent logic
python3 ~/agents/my_agent.py --task "$TASK"
```

Configure:
```toml
[agent_clis.my-agent]
name = "my-agent"
command = "~/bin/my-agent.sh"
```

## Example: Docker Container

```toml
[agent_clis.docker-agent]
name = "docker-agent"
command = "docker run --rm -v $(pwd):/workspace my-agent:latest"
```

## Programmatic Model Registration

In Rust code:

```rust
use tinytown::agent::AgentModel;

// Create custom model
let model = AgentModel::new("my-model", "my-command --flag")
    .with_workdir("/path/to/workdir")
    .with_env("API_KEY", "secret");

// Models are used when spawning
town.spawn_agent("worker", "my-model").await?;
```

## Best Practices

1. **Use absolute paths** — Relative paths may break
2. **Handle stdin/stdout** — Agents should read tasks from messages
3. **Set timeouts** — Don't let agents run forever
4. **Log output** — Direct to `logs/` directory
5. **Test locally first** — Before adding to config

## Troubleshooting

### Command Not Found

```bash
# Check the command works directly
/path/to/my/agent --help
```

### Environment Variables Not Set

```bash
# Debug by adding echo
"command": "env && /path/to/agent"
```

### Working Directory Issues

Use absolute paths:
```toml
workdir = "/Users/you/agents"
```

Not:
```toml
workdir = "./agents"  # May not resolve correctly
```

