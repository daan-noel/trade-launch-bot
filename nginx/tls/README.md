# TLS certs for nginx (local / testing)

The `web` (nginx) container terminates TLS using `cert.pem` + `key.pem` in this
directory. They are **self-signed** and meant for local/testing only. The `.pem`
files are gitignored (never commit a private key), so you must generate them
once before `docker compose up --build`.

## Generate

From the repo root:

```bash
openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
  -keyout nginx/tls/key.pem \
  -out    nginx/tls/cert.pem \
  -subj "/CN=localhost" \
  -addext "subjectAltName=DNS:localhost,IP:127.0.0.1,IP:172.31.46.217"
```

On Git Bash for Windows, prefix the command with `MSYS_NO_PATHCONV=1` so the
`-subj` value isn't rewritten into a Windows path.

## Notes

- Browsers will show a "not trusted / self-signed" warning — expected. Click
  through it for local testing.
- Add more SANs (extra hostnames/IPs) to `subjectAltName` if you reach the app
  by something other than `localhost` / `127.0.0.1`.
- For production, replace these with a real cert (e.g. Let's Encrypt) — do not
  ship a self-signed cert to users.
