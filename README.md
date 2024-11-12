Trashmap lets you start a [DDraceNetwork](https://ddnet.org/) server for testing your maps at a click of a button. The name is inspired by TrashMail. A number of [public instances](https://wiki.ddnet.org/wiki/Trashmap#Public_instances) are hosted around the world by the community.

## Deployment

When built in release mode the HTML and JS files are baked into the binary. Simply deploy the service behind an Nginx reverse proxy or similar. If using Nginx, you may want to set `client_max_body_size 10m;` to allow map uploads as large as 10 MB.

If the service is managed by a systemd service unit, you may want to set `OOMPolicy=continue` in the `[Service]` section to keep the unit running when a DDNet-Server process is killed by the kernel due to memory pressure.

## Building

If the glibc version of the machine you compile on and the server you want to deploy on don't match, you may run into compatibility issues. In this case you may choose to statically link against musl libc. Install the musl target using rustup or your distros package manager and build as follows:

```
cargo build --release --target x86_64-unknown-linux-musl
```

You will find the binary at `target/x86_64-unknown-linux-musl/release/trashmap`.

## Configuration

Run the binary once to learn the expected location of the config file on your system. Here is an example:

```toml
http_port = 3000
executable_path = "/usr/bin/DDNet-Server"
port_range = [8303, 8310]
public_address = "trashmap.ddnet.org"
```
