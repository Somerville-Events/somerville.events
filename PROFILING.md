# Profiling Guide

This project uses [samply](https://github.com/mstange/samply) for statistical CPU profiling and [Apache Bench (ab)](https://httpd.apache.org/docs/2.4/programs/ab.html) for load generation.

## Prerequisites

You will need `samply` and `ab` installed.

```bash
# macOS
brew install samply

# Linux / Cargo
cargo install samply
# ab is usually part of apache2-utils or httpd-tools
sudo apt-get install apache2-utils
```

## Quick Start

We have an automated script that builds the project in release mode, starts the profiler, runs a load test, and saves the results.

```bash
./profile.sh
```

## Viewing Results

The script produces a file named `profile.json`. You can view it in two ways:

1.  **Command Line (Recommended)**:

    ```bash
    samply load profile.json
    ```

    This will automatically open the profile in your default web browser.

2.  **Web Upload**:
    Go to [profiler.firefox.com](https://profiler.firefox.com/) and upload the `profile.json` file manually.

## Manual Profiling

If you want to run the steps manually without the script:

1.  **Build in Release Mode**:

    ```bash
    cargo build --release
    ```

2.  **Run with Samply**:

    ```bash
    # --save-only avoids opening the browser immediately
    samply record --save-only -o profile.json -- ./target/release/somerville-events
    ```

3.  **Generate Load** (in a separate terminal):

    ```bash
    # 2000 requests, 20 concurrent
    ab -n 2000 -c 20 http://127.0.0.1:8080/
    ```

4.  **Stop Samply**:
    Hit `Ctrl+C` in the terminal running `samply`.

## Interpreting the Profile

- **Timeline**: Shows CPU usage over time.
- **Call Tree**: Shows the hierarchy of function calls and where time was spent (aggregated).
- **Flame Graph**: Visual representation of the call stack frequency.

Look for "wide" blocks in the Flame Graph or high percentages in the Call Tree that correspond to your application logic (e.g., `somerville_events::...`). Ignore idle time or system calls unless they are unexpected.
