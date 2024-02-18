# spam-musubi

![image](https://upload.wikimedia.org/wikipedia/commons/thumb/3/31/Homemade_Spam_Musubi.jpg/640px-Homemade_Spam_Musubi.jpg)

Layer7 firewall for ActivityPub-compatible servers. IPv4 only.

> IMPORTANT: your reverse proxy must be using HTTP/1.0 after TLS termination. spam-musubi doesn't support fancy HTTP.

<details>
    <summary>Sample nginx config snippet</summary>
```
server {
    listen 443 ssl http2 default_server;
    server_name activitypub.rocks;

    --snip--

    client_max_body_size 100M;

    --snip--

    # websocket should NOT be proxied through spam-musubi
    location /streaming {
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_pass "http://localhost:3000/streaming";
    }

    # everything else
    location / {
        --snip--
        proxy_http_version 1.0;
        proxy_pass http://localhost:21200/;
    }
}
```
</details>

## How to use

> I assume you are using nginx or some sort of reverse proxy.
>
> If you aren't, you should use one to limit request payload size, etc.

- Currently supports Misskey only, but adding support for other server is  trivial - send me PR. (See `src/query/constants.rs`)

- Install rustup from <https://rustup.rs/>

- Clone the repo, build:

```
git clone https://gitlab.com/chocological00/spam-musubi.git
cd spam-musubi
cargo build --release
```

- Run `cargo run --release -- --help` and find out what args you'll need.

- At this stage, I would recommend testing it (run spam-musubi on tmux, and temporaily change nginx settings), before you make it permanent using systemd daemons below.

- Particularly, ensure that your server can receive non-spam notes from other instances. If you see false positives, 99% of the time it means that follower / following count checking with DB is bad. You can run with `RUST_LOG=debug cargo run --release` to see which step went wrong.

- As sudo, create a new systemd daemon:

```
# cat /etc/systemd/system/spam-musubi.service
[Unit]
Description=Spam Musubi

[Service]
Type=simple
User=<your username here>
ExecStart=/<path to repo>/target/release/spam-musubi
WorkingDirectory=/<path to repo>
Environment="DB_HOST=ip_address_to_db"
Environment="DB_PORT=5432"
Environment="DB_USER=your_db_username"
Environment="DB_PASSWORD=hunter2"
Environment="DB_NAME=misskey"
TimeoutSec=60
StandardOutput=syslog
StandardError=syslog
SyslogIdentifier=spam-musubi
Restart=always

[Install]
WantedBy=multi-user.target
```

- still as sudo, enable daemon:

```
systemctl daemon-reload
systemctl enable spam-musubi
systemctl start spam-musubi
```

- In your nginx settings, change the `proxy_pass` to point to spam-musubi. (port 21200 by default)

- Run `nginx -t && systemctl restart nginx` as sudo to apply nginx changes. 

> NOTE: it is not recommended to proxy websockets through spam_musubi

## How to update
- Once you have systemd daemon set up, updating is easy!

```
cd <path to repo>
git pull
cargo build --release
sudo systemctl restart spam-musubi
```

---

Icon By Chris Hackmann - Own work, CC BY-SA 4.0, https://commons.wikimedia.org/w/index.php?curid=43387131
