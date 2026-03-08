# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0](https://github.com/jeremyplichta/tinytown/compare/v0.1.0...v0.2.0) (2026-03-08)


### Features

* Add agent stats (rounds completed, uptime) ([4b471ba](https://github.com/jeremyplichta/tinytown/commit/4b471baeba2d574b625c98a1c2589a9835a46122))
* Add conductor mode, task planning DSL, and default model ([ce5bce0](https://github.com/jeremyplichta/tinytown/commit/ce5bce0deb4f8dbb90202b1aa884899ed82cc771))
* Add deep status with bounded, TTL'd activity logs ([3d4a5e4](https://github.com/jeremyplichta/tinytown/commit/3d4a5e4faeb10c62746317466e676bec0b8687af))
* Add Redis AOF save/restore for version control ([464f868](https://github.com/jeremyplichta/tinytown/commit/464f868f55d210eb581a98d6db9f5e2b3ae2fb45))
* Add tt bootstrap to download and build Redis via AI agent ([580dc0c](https://github.com/jeremyplichta/tinytown/commit/580dc0c031e43f8d50b2d5a01daee95c16b2d1bc))
* Add tt kill for graceful agent cleanup ([1545a6d](https://github.com/jeremyplichta/tinytown/commit/1545a6d285672add0f25d512cf6d7c5b506f2957))
* Add urgent message queue for priority interrupts ([497250d](https://github.com/jeremyplichta/tinytown/commit/497250de6b01e3901706617d4099d709171cc1ac))
* Agent prompt instructs to keep checking inbox until empty ([0de8a7c](https://github.com/jeremyplichta/tinytown/commit/0de8a7c8700360b37728fc3605ba5212a83ce30f))
* Agents are real processes with supervisor loop ([351a22f](https://github.com/jeremyplichta/tinytown/commit/351a22fa802918dbfe1be4a832f4f9d55c1053d7))
* Auto-derive town name from git repo and branch ([71be085](https://github.com/jeremyplichta/tinytown/commit/71be085e062125205a52b81959c619dfab115690))
* Conductor is context-aware and suggests team roles ([1eff77c](https://github.com/jeremyplichta/tinytown/commit/1eff77c2abf6b7807258864992bbe77d1a6248ab))
* Conductor saves state to tasks.toml for git ([6c9a513](https://github.com/jeremyplichta/tinytown/commit/6c9a513ea9bec6cfff2e1431e1e2984ced4f796e))


### Bug Fixes

* Update CLI commands with correct non-interactive flags ([bfb9608](https://github.com/jeremyplichta/tinytown/commit/bfb9608d5649aa4c1403242daeb1c30b0e12705f))

## 0.1.0 (2026-03-08)


### ⚠ BREAKING CHANGES

* Initial release

### Features

* Initial release of Tinytown v0.1.0 ([c3a693a](https://github.com/jeremyplichta/tinytown/commit/c3a693aede5c5dd8a0fb344aa2742ae4082cd6fa))

## [Unreleased]

## [0.1.0] - 2024-03-08

### Features

- Initial release of Tinytown
- 5 core types: Town, Agent, Task, Message, Channel
- Redis-based message passing with Unix socket support
- CLI tool `tt` with commands: init, spawn, assign, list, status, start, stop
- Built-in presets for Claude, Auggie, Codex, Gemini, Copilot, Aider, Cursor
- Redis 8.0+ version checking
- Comprehensive test suite (32 integration tests)

### Documentation

- README with quick start guide and architecture overview
- Complexity comparison with Gastown
- Redis 8.0+ installation instructions
