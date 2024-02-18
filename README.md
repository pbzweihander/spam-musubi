# spam-musubi

Layer7 firewall for ActivityPub-compatible servers. IPv4 only.

## How to use

> I assume you are using nginx or some sort of reverse proxy. If you aren't, you should use one to limit request payload size, etc.

In your nginx configuration, find the part where you're proxying to the AP server.
This is your "inside port" (default 3000 for Misskey).
