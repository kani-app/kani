# Install from a Binary

## Download and verify

Open [GitHub Releases](https://github.com/kani-app/kani/releases) and download the archive for your
platform. Archive names and supported targets are recorded on each release; do not reuse a
filename from another release without checking it. Verify the checksums supplied with the release
before extracting it.

The server executable is named `kani-web`. Releases may also contain `kani-cli`, which is used for
extension and development workflows rather than for running the service.

The web interface is built into `kani-web`; there is no separate asset download and nothing else to
place on disk. To serve a modified frontend instead, point `KANI_STATIC_DIR` at a directory holding
it — otherwise leave the variable unset.

## Configure directories

Kani uses environment variables for boot and infrastructure concerns and the web settings UI for
runtime behavior. There is no `kani.toml` configuration file.

Create writable data and library directories, then set their locations:

```bash
export KANI_DATA_DIR=/var/lib/kani
export KANI_LIBRARY_DIR=/var/lib/kani/library
export KANI_BIND=127.0.0.1:8242
```

On first start Kani creates `kani.db`, `secret.key`, and `proxy.key` in the data directory. Protect
and back up all three. See [Configuration](../admin/configuration.md) before exposing the service.

## Run with systemd

Create `/etc/systemd/system/kani.service`:

```ini
[Unit]
Description=Kani manga server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=kani
Group=kani
WorkingDirectory=/var/lib/kani
Environment=KANI_DATA_DIR=/var/lib/kani
Environment=KANI_LIBRARY_DIR=/var/lib/kani/library
Environment=KANI_BIND=127.0.0.1:8242
ExecStart=/usr/local/bin/kani-web
Restart=on-failure
RestartForceExitStatus=42
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

Create the service account and directories, install the binary, then enable the unit:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now kani
sudo journalctl -u kani -f
```

## Build from source

Install the prerequisites from [Local development](../developer/local-dev.md), then run:

```bash
cargo run -p kani-cli -- setup
cargo build --release -p kani-web
```

The binary is written to `target/release/kani-web`.
