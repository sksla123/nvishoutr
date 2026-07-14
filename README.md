# nvishoutr

`nvishoutr` watches NVIDIA GPUs on a shared server and shouts when usable GPU
capacity opens up.

It is meant for the common lab/team setup where one GPU machine is shared by
several people, everyone is waiting for a free card, and nobody wants to keep
typing `nvidia-smi` all evening. `nvishoutr` polls selected GPUs, ignores
display/GUI processes such as `gdm`, `Xorg`, and `gnome-shell`, and sends a
notification when the configured free condition is met.

Notifications are delivered through
[shoutrrr](https://crates.io/crates/shoutrrr), so one event can fan out to
multiple destinations such as Discord, Slack, or a generic webhook.

## Features

- Watch all GPUs or a selected list such as `0,1,3`
- Notify when any watched GPU is free, or only when all watched GPUs are free
- Ignore GUI/display processes by default
- Send reminder notifications while a GPU stays free
- Send a busy-again notification when a previously free GPU is taken
- Keep Discord markdown tables and code blocks in a single message
- Minimal CLI: `--config`, `--gpus`, and `--dry-run`

## Install

Build from source:

```bash
cargo build --release
```

Run the built binary:

```bash
./target/release/nvishoutr --config cfgs/example.yaml --gpus all
```

Or run directly with Cargo:

```bash
cargo run --release -- --config cfgs/example.yaml --gpus all
```

## Config

Create your own YAML config from [cfgs/example.yaml](cfgs/example.yaml):

```yaml
notify_urls:
  - "discord://WEBHOOK_TOKEN@WEBHOOK_ID?splitLines=No"
notify_when: "any"
notify_busy_again: true
poll_interval_seconds: 60
reminder_interval_minutes: 30
notification_title: "GPU free"
nvidia_smi: "nvidia-smi"

ignore:
  process_patterns: []
  graphics_users: []
```

`notify_urls` is required and accepts one or more shoutrrr URLs. Discord URLs
should include `splitLines=No` so markdown tables and code blocks stay in one
message. `nvishoutr` also adds it automatically when it is missing.

Keep real configs with webhook tokens out of git. This repository ignores
`cfgs/*.yaml` except `cfgs/example.yaml`.

## Notification Modes

`notify_when: "any"` sends a separate notification when any watched GPU becomes
free.

```bash
./target/release/nvishoutr --config cfgs/my-server.yaml --gpus 0,1,3
```

`notify_when: "all"` sends one grouped notification only when every watched GPU
is free.

```bash
./target/release/nvishoutr --config cfgs/my-server.yaml --gpus all
```

`notify_busy_again: true` sends one warning if a GPU that was already reported
free becomes busy again before the next reminder.

## CLI

```bash
nvishoutr --config <PATH> --gpus <all|GPU_LIST> [--dry-run]
```

Examples:

```bash
nvishoutr --config cfgs/my-server.yaml --gpus all
nvishoutr --config cfgs/my-server.yaml --gpus 0,1,3
nvishoutr --config cfgs/my-server.yaml --gpus 0 --dry-run
```

## Notification Shape

A free GPU notification includes:

- GPU index and host
- GPU name
- GPU utilization and memory utilization
- VRAM used, total, and free
- Active compute processes, if any
- Ignored GUI/display processes

Example:

```markdown
✅ **GPU is free**
`GPU:0` · `shared-gpu-01` · `2026-07-14 09:30:00 KST`

**GPU status**
Name      Tesla V100-DGXS-32GB
Util      GPU 0% | MEM 0%
VRAM      22MiB/32768MiB (32471MiB free)
Compute   No running processes
```

## Release

CI runs on pushes and pull requests:

- `cargo fmt --check`
- `cargo test --locked`
- `cargo clippy --locked -- -D warnings`

Release runs on version tags after the same checks pass. To publish `0.1.0`:

```bash
git add .
git commit -m "Release nvishoutr 0.1.0"
git tag v0.1.0
git push origin main
git push origin v0.1.0
```

The release workflow creates:

- `nvishoutr-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`
- `nvishoutr-v0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256`
- `release/v0.1.0.md`

The markdown file at [release/v0.1.0.md](release/v0.1.0.md) is used as the
GitHub Release body and is also attached as a release asset.

## Notes

`nvishoutr` reads GPU state through `nvidia-smi -q -x`, so NVIDIA drivers and
`nvidia-smi` must be available on the server.

This tool does not reserve GPUs. It only notifies you when a watched GPU appears
free.
