# spam-musubi

![image](https://upload.wikimedia.org/wikipedia/commons/thumb/3/31/Homemade_Spam_Musubi.jpg/640px-Homemade_Spam_Musubi.jpg)

Layer7 firewall for ActivityPub-compatible servers. IPv4 only.

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

- As sudo, create a new systemd daemon:

```
# cat /etc/systemd/system/spam-musubi.service
[Unit]
Description=Spam Musubi

[Service]
Type=simple
User=yourusernamehere
ExecStart=/pathtorepo/target/release/spam-musubi
WorkingDirectory=/pathtorepo
Environment="DB_HOST=ip_address_to_db"
Environment="DB_PORT=5432"
Environment="DB_USER=your_db_username"
Environment="DB_PASSWORD=hunter2"
Environment="DB_NAME=misskey"
TimeoutSec=60
StandardOutput=syslog
StandardError=syslog
SyslogIdentifier=spammusubi
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

> NOTE: it is not recommended to proxy websockets through spam_musubi

---

Icon By Chris Hackmann - Own work, CC BY-SA 4.0, https://commons.wikimedia.org/w/index.php?curid=43387131
