# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
