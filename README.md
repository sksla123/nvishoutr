# nvishoutr

`nvishoutr` is a lightweight, robust NVIDIA GPU availability monitoring daemon written in Rust. It continuously monitors GPU workloads and dispatches real-time webhook notifications (e.g., Discord) when GPUs become free or busy again.

## Installation & Quick Start

### 1. Installation

#### Option A: Download Pre-built Release Binary

Download the statically compiled, standalone `musl` binary directly from GitHub Releases without requiring a Rust toolchain or GLIBC runtime dependencies:

```bash
# Download latest static release (replace v0.1.2 with target version)
curl -LO https://github.com/your-org/nvishoutr/releases/download/v0.1.2/nvishoutr-x86_64-unknown-linux-musl.tar.gz

# Extract and install binary to local path
tar -xzf nvishoutr-x86_64-unknown-linux-musl.tar.gz
chmod +x nvishoutr
sudo mv nvishoutr /usr/local/bin/
```

#### Option B: Build from Source with Cargo

Build the release binary directly on your machine:

```bash
# 1. Clone the repository
git clone https://github.com/your-org/nvishoutr.git
cd nvishoutr

# 2. Build optimized release binary
cargo build --release

# The compiled binary is available at:
./target/release/nvishoutr --version
```

### 2. GPU Selection & Monitoring Modes (`--gpus`)

`nvishoutr` provides granular control over which GPUs to track, evaluated against your configured notification trigger mode (`notify_when: any` or `notify_when: all`).

#### GPU Selection Flag (`--gpus`)
* `--gpus all`: Monitors every GPU indexed by `nvidia-smi`.
* `--gpus 0,1`: Monitors only specific GPUs (comma-separated indices).

#### Notification Trigger Modes (Configured in YAML)
* **`notify_when: any`**: Sends a notification as soon as **at least one** tracked GPU becomes free. Useful when you need immediate access to single-GPU resources.
* **`notify_when: all`**: Sends a notification only when **all** selected GPUs become free concurrently. Essential for distributed multi-GPU training jobs.

#### Example Commands:
```bash
# Watch all GPUs and alert when any single GPU is freed
./target/release/nvishoutr --config cfgs/server-any.yaml --gpus all

# Watch all GPUs and alert only when the entire cluster/node is completely free
./target/release/nvishoutr --config cfgs/server-all.yaml --gpus all

# Watch only GPU 0 and GPU 1
./target/release/nvishoutr --config cfgs/server.yaml --gpus 0,1
```

### 3. Verification Mode (`--verify-config`)

Verifies your YAML configuration file syntax and validates network connectivity by attempting an actual test dispatch to all configured webhook endpoints.

* **Role**: Ensures URLs, authentication tokens, and timeouts are operational before starting the long-running daemon.
* **Usage**:
  ```bash
  ./target/release/nvishoutr --config cfgs/server.yaml --gpus all --verify-config
  ```
* **Exit Code**: Returns `0` on successful delivery, or non-zero with error diagnostics if connection fails.

### 4. Dry Run Mode (`--dry-run`)

Runs the monitoring daemon with active GPU status detection and console output, but **suppresses all actual network notification dispatches**.

* **Role**: Allows verification of process ignore filters, polling intervals, and terminal status UI behavior without spamming destination channels or bots.
* **Usage**:
  ```bash
  ./target/release/nvishoutr --config cfgs/server.yaml --gpus all --dry-run
  ```

## Configuration (`config.yaml`)

Configuration is managed via YAML. All notification targets use Shoutrrr URL schemes (such as `discord://<token>@<id>`).

### Example Configuration

```yaml
# Destination webhooks
notify_urls:
  - "discord://YOUR_WEBHOOK_TOKEN@YOUR_WEBHOOK_ID?splitLines=No"

# Notification trigger policy: "any" or "all"
notify_when: "all"

# Re-notify when GPUs transition back from free to busy
notify_busy_again: true

# Polling and reminder timers
poll_interval_seconds: 10.0
reminder_interval_minutes: 30.0

# Network failure resilience (optional)
notify_retry_count: 3             # Number of retries on transient webhook failures (default: 3)
notify_retry_delay_seconds: 2.0   # Delay in seconds between retries (default: 2.0)

# Notification header title
notification_title: "GPU Cluster Status"

# Path to nvidia-smi binary
nvidia_smi: "nvidia-smi"

# Process exclusion rules
ignore:
  process_patterns:
    - "^Xorg$"
    - "^gnome-shell$"
  graphics_users:
    - "gdm"
```

### Parameter Reference

| Field | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `notify_urls` | `list[str]` | **Required** | Shoutrrr-compatible webhook URLs. |
| `notify_when` | `string` | `"any"` | Trigger condition: `"any"` (at least 1 free) or `"all"` (all free). |
| `notify_busy_again` | `boolean` | `true` | Send notification when free GPUs become occupied again. |
| `poll_interval_seconds` | `float` | `60.0` | Interval between `nvidia-smi` queries. |
| `reminder_interval_minutes` | `float` | `30.0` | Periodic reminder interval while GPU state remains free. |
| `notify_retry_count` | `integer` | `3` | Maximum retry attempts if notification webhook fails. |
| `notify_retry_delay_seconds` | `float` | `2.0` | Delay between retry attempts. |
| `notification_title` | `string` | `"GPU free"` | Title prefix in notifications. |
| `nvidia_smi` | `string` | `"nvidia-smi"` | Path or command name for `nvidia-smi`. |
| `ignore.process_patterns` | `list[str]` | `[]` | Regex patterns for process names to ignore (e.g., desktop managers). |
| `ignore.graphics_users` | `list[str]` | `[]` | System user accounts whose GPU processes are excluded from busy status. |

> **Note on Process Filtering (`ignore`)**:  
> The `ignore` section shown above is provided as an illustrative example. Common desktop environment display servers and display manager daemons (such as `Xorg`, `gnome-shell`, and the `gdm` user account) are filtered out by default internally to prevent false-busy states on display-attached GPUs. You only need to declare custom patterns or user accounts here if your environment uses non-standard window managers or background services.
