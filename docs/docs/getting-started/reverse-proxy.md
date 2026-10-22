# Reverse Proxy

Kani serves HTTP. Terminate TLS at a reverse proxy for internet-facing deployments and set
`KANI_SECURE_COOKIES=true`. Restrict direct access to port 8242 using the host firewall, loopback
binding, or a private container network.

## nginx

```nginx
server {
    listen 443 ssl http2;
    server_name kani.example.com;

    ssl_certificate     /etc/letsencrypt/live/kani.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/kani.example.com/privkey.pem;

    client_max_body_size 100M;

    location / {
        proxy_pass         http://127.0.0.1:8242;
        proxy_set_header   Host $host;
        proxy_set_header   X-Real-IP $remote_addr;
        proxy_set_header   X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header   X-Forwarded-Proto $scheme;
        proxy_http_version 1.1;
        proxy_read_timeout 300s;
        proxy_buffering off;
    }
}
```

## Caddy

```caddy
kani.example.com {
    reverse_proxy 127.0.0.1:8242
}
```

Caddy obtains and renews TLS certificates automatically.

## Traefik

When Kani and Traefik share a Docker network, labels can route to the container's port:

```yaml
services:
  kani:
    labels:
      - traefik.enable=true
      - traefik.http.routers.kani.rule=Host(`kani.example.com`)
      - traefik.http.routers.kani.entrypoints=websecure
      - traefik.http.routers.kani.tls.certresolver=letsencrypt
      - traefik.http.services.kani.loadbalancer.server.port=8242
```

## Proxy checklist

- Preserve the original host and scheme so redirects, cookies, and absolute links use the public
  origin.
- Disable response buffering for Server-Sent Events; Kani uses SSE for progress and invalidation.
- Increase the request-body limit if users import large backup or CBZ files.
- Allow long responses for exports and large downloads.
- Set `KANI_CORS_ORIGIN=https://kani.example.com` when browser requests should be accepted only
  from the public origin.
- Complete first-run setup before publishing the route. A reverse proxy is part of the trust
  boundary because its address may be the peer Kani sees.

Enable `KANI_PUBLIC_INSTANCE=true` for an internet-facing deployment and review
[Security hardening](../admin/security-hardening.md).
