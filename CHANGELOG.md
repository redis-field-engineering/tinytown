# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.8.1](https://github.com/redis-field-engineering/tinytown/compare/v0.8.0...v0.8.1) (2026-03-14)


### Bug Fixes

* **ci:** update release-please workflow wiring ([396b873](https://github.com/redis-field-engineering/tinytown/commit/396b873f3f1ee368a14bbfbd4f7d5d60782f96de))


### Documentation

* Add mission mode documentation and REST API spec ([#34](https://github.com/redis-field-engineering/tinytown/issues/34)) ([48445db](https://github.com/redis-field-engineering/tinytown/commit/48445dbeff936dd388e111cf35cf50881a4c83f4))

## [0.8.0](https://github.com/redis-field-engineering/tinytown/compare/v0.7.0...v0.8.0) (2026-03-10)


### Features

* Add autonomous multi-issue mission mode ([#32](https://github.com/redis-field-engineering/tinytown/issues/32)) ([8efd422](https://github.com/redis-field-engineering/tinytown/commit/8efd422eebcf07cc67f2ee304966065ca868077b)), closes [#23](https://github.com/redis-field-engineering/tinytown/issues/23)

## [0.7.0](https://github.com/redis-field-engineering/tinytown/compare/v0.6.0...v0.7.0) (2026-03-09)


### Features

* implement issues [#18](https://github.com/redis-field-engineering/tinytown/issues/18), [#19](https://github.com/redis-field-engineering/tinytown/issues/19), [#21](https://github.com/redis-field-engineering/tinytown/issues/21) - CLI improvements ([#29](https://github.com/redis-field-engineering/tinytown/issues/29)) ([4ee5ad4](https://github.com/redis-field-engineering/tinytown/commit/4ee5ad4456bf8ccc146d461f000f59290a8b855e))

## [0.6.0](https://github.com/redis-field-engineering/tinytown/compare/v0.5.0...v0.6.0) (2026-03-09)


### Features

* **townhall:** add authentication, authorization, and audit logging ([#24](https://github.com/redis-field-engineering/tinytown/issues/24)) ([8150f14](https://github.com/redis-field-engineering/tinytown/commit/8150f144aa005645e2561fa43223a639100c6863))
* **townhall:** Add MCP interface ([#17](https://github.com/redis-field-engineering/tinytown/issues/17)) ([#26](https://github.com/redis-field-engineering/tinytown/issues/26)) ([0e267fe](https://github.com/redis-field-engineering/tinytown/commit/0e267fe0ad5d7e0fbb28f53474842ba74c55190a))
* **townhall:** Introduce townhall REST control plane daemon ([#20](https://github.com/redis-field-engineering/tinytown/issues/20)) ([c08dcc5](https://github.com/redis-field-engineering/tinytown/commit/c08dcc586a25a1f3527ab630df62b7ffba658c5e)), closes [#15](https://github.com/redis-field-engineering/tinytown/issues/15)

## [0.5.0](https://github.com/redis-field-engineering/tinytown/compare/v0.4.0...v0.5.0) (2026-03-08)


### ⚠ BREAKING CHANGES

* **config:** tt init now uses global config defaults instead of hardcoded values

### Features

* **cli:** initialize global config and improve round tracking ([e2c293e](https://github.com/redis-field-engineering/tinytown/commit/e2c293eccf9f2319a1f46f539a9aa84515e2a717))
* **config:** add central Redis configuration to GlobalConfig ([da00ab7](https://github.com/redis-field-engineering/tinytown/commit/da00ab7222485d18bf5aa837e470cd9695055519))
* **config:** use GlobalConfig defaults for new towns ([0be06c6](https://github.com/redis-field-engineering/tinytown/commit/0be06c6776b1cab4137f1b0db11c73ce4b5b6dda))
* **town:** support central Redis instance shared across towns ([fb74188](https://github.com/redis-field-engineering/tinytown/commit/fb74188b8614a4fa75c4c6d5b763079f162d5d5a))


### Bug Fixes

* address additional Cursor Bugbot security concerns ([d170ba6](https://github.com/redis-field-engineering/tinytown/commit/d170ba6032ffc09fc35429ad8d285138043df5c2))
* address Cursor Bugbot review comments ([4cc6f14](https://github.com/redis-field-engineering/tinytown/commit/4cc6f141d614c21b14d7287547eaa257e930afe9))

## [0.4.0](https://github.com/redis-field-engineering/tinytown/compare/v0.3.0...v0.4.0) (2026-03-08)


### Features

* add RedisManager for centralized Redis instance ([bbca6a1](https://github.com/redis-field-engineering/tinytown/commit/bbca6a151c78d202163e94019fd1c099faa13829))
* **cli:** add 'tt recover' command for orphaned agents ([cc18c70](https://github.com/redis-field-engineering/tinytown/commit/cc18c70f445a08d074e50290d969d937533b78bd))
* **cli:** add global town registry in ~/.tt/towns.toml ([c5ab161](https://github.com/redis-field-engineering/tinytown/commit/c5ab1615d78b4ac36d10695aaeaa4840798d2606))
* **cli:** add task backlog queue commands ([87030b6](https://github.com/redis-field-engineering/tinytown/commit/87030b667fc28a0d3dfd09c8e76daa8bd72b77c2))
* **redis:** add TCP support with password authentication ([7538924](https://github.com/redis-field-engineering/tinytown/commit/75389242512a9f6df8d43e268b0f266c27d4f877))


### Bug Fixes

* **tests:** add Redis cleanup to prevent process leaks ([d51d096](https://github.com/redis-field-engineering/tinytown/commit/d51d0967e243d5bc42d72c5b1b55d0240aec1ed6))

## [0.3.0](https://github.com/redis-field-engineering/tinytown/compare/v0.2.0...v0.3.0) (2026-03-08)


### Features

* add global config and rename model to cli ([c679481](https://github.com/redis-field-engineering/tinytown/commit/c67948174903159518c33d23c1acf294f46831d8))
* auto-detect Redis in ~/.tt/bin ([9b11fdf](https://github.com/redis-field-engineering/tinytown/commit/9b11fdf559a2aa376b19f00ef94792e37c5b6109))

## [0.2.0](https://github.com/redis-field-engineering/tinytown/compare/v0.1.0...v0.2.0) (2026-03-08)


### Features

* Add agent stats (rounds completed, uptime) ([4b471ba](https://github.com/redis-field-engineering/tinytown/commit/4b471baeba2d574b625c98a1c2589a9835a46122))
* Add conductor mode, task planning DSL, and default model ([ce5bce0](https://github.com/redis-field-engineering/tinytown/commit/ce5bce0deb4f8dbb90202b1aa884899ed82cc771))
* Add deep status with bounded, TTL'd activity logs ([3d4a5e4](https://github.com/redis-field-engineering/tinytown/commit/3d4a5e4faeb10c62746317466e676bec0b8687af))
* Add Redis AOF save/restore for version control ([464f868](https://github.com/redis-field-engineering/tinytown/commit/464f868f55d210eb581a98d6db9f5e2b3ae2fb45))
* Add tt bootstrap to download and build Redis via AI agent ([580dc0c](https://github.com/redis-field-engineering/tinytown/commit/580dc0c031e43f8d50b2d5a01daee95c16b2d1bc))
* Add tt kill for graceful agent cleanup ([1545a6d](https://github.com/redis-field-engineering/tinytown/commit/1545a6d285672add0f25d512cf6d7c5b506f2957))
* Add urgent message queue for priority interrupts ([497250d](https://github.com/redis-field-engineering/tinytown/commit/497250de6b01e3901706617d4099d709171cc1ac))
* Agent prompt instructs to keep checking inbox until empty ([0de8a7c](https://github.com/redis-field-engineering/tinytown/commit/0de8a7c8700360b37728fc3605ba5212a83ce30f))
* Agents are real processes with supervisor loop ([351a22f](https://github.com/redis-field-engineering/tinytown/commit/351a22fa802918dbfe1be4a832f4f9d55c1053d7))
* Auto-derive town name from git repo and branch ([71be085](https://github.com/redis-field-engineering/tinytown/commit/71be085e062125205a52b81959c619dfab115690))
* Conductor is context-aware and suggests team roles ([1eff77c](https://github.com/redis-field-engineering/tinytown/commit/1eff77c2abf6b7807258864992bbe77d1a6248ab))
* Conductor saves state to tasks.toml for git ([6c9a513](https://github.com/redis-field-engineering/tinytown/commit/6c9a513ea9bec6cfff2e1431e1e2984ced4f796e))


### Bug Fixes

* Update CLI commands with correct non-interactive flags ([bfb9608](https://github.com/redis-field-engineering/tinytown/commit/bfb9608d5649aa4c1403242daeb1c30b0e12705f))
* update default branch to main and add quick install instructions ([37a1bb6](https://github.com/redis-field-engineering/tinytown/commit/37a1bb6b4cb65021190771167ff07822b57940bc))

## 0.1.0 (2026-03-08)


### ⚠ BREAKING CHANGES

* Initial release

### Features

* Initial release of Tinytown v0.1.0 ([c3a693a](https://github.com/redis-field-engineering/tinytown/commit/c3a693aede5c5dd8a0fb344aa2742ae4082cd6fa))

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
